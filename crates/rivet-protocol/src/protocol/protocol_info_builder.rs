//! Port of `net.minecraft.network.protocol.ProtocolInfoBuilder` (MC 26.2).
//!
//! Java: `ProtocolInfoBuilder.java` in `working/Paper` (vanilla 26.2). The
//! registration surface: `addPacket` in `addPacket`-call order assigns network
//! ids (`IdDispatchCodec` ids are registration order), `withBundlePacket`
//! registers the bundle delimiter (unit codec, network id 0) and records the
//! `BundlerInfo`, and `buildUnbound`/`buildUnbound()` freeze the builder into a
//! template. The static `serverboundProtocol`/`clientboundProtocol`/
//! `contextServerboundProtocol`/`contextClientboundProtocol` helpers mirror
//! Java's one-liners that produce the `*Protocols.TEMPLATE` values.
//!
//! The `CodecModifier` overload (3-arg `addPacket`) and the context application
//! it needs are deferred with the registry-wired codecs (#126/#109); the
//! `context*` helpers and [`crate::protocol::UnboundProtocol`] still exist so
//! the play protocols can adopt them without a surface change, but until
//! modifiers land the context is carried but unused.
//!
//! RivetTodo(#126): the `CodecModifier` overload + context application are not
//! ported (registry-wired codecs); the context is carried but unused.

use crate::codec::{StreamCodec, unit};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::generated::protocol::{ConnectionProtocol, PacketFlow};
use crate::protocol::bundle::BundlerInfo;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::protocol_codec_builder::ProtocolCodecBuilder;
use crate::protocol::simple_unbound_protocol::SimpleUnboundProtocol;
use crate::protocol::unbound_protocol::UnboundProtocol;
use crate::protocol_info::{ProtocolDetails, ProtocolInfo};
use std::fmt::Display;
use std::marker::PhantomData;

/// One `addPacket` entry: the packet's discriminator and its body codec.
struct CodecEntry<V: 'static> {
    ty: PacketType,
    codec: StreamCodec<FriendlyByteBuf, V>,
}

/// `net.minecraft.network.protocol.ProtocolInfoBuilder<T, B, C>` with
/// `B = FriendlyByteBuf` and the packet value `T` erased to `V`.
pub struct ProtocolInfoBuilder<V: 'static, C> {
    protocol: ConnectionProtocol,
    flow: PacketFlow,
    codecs: Vec<CodecEntry<V>>,
    bundler_info: Option<BundlerInfo>,
    /// `C` is carried so the `context*Protocol` helpers typecheck; the
    /// `fn(C)` phantom avoids a spurious `C: Clone` bound on derived `Clone`.
    _context: PhantomData<fn(C)>,
}

impl<V: 'static, C> ProtocolInfoBuilder<V, C> {
    /// `new ProtocolInfoBuilder<>(ConnectionProtocol protocol, PacketFlow flow)`.
    pub fn new(protocol: ConnectionProtocol, flow: PacketFlow) -> Self {
        ProtocolInfoBuilder {
            protocol,
            flow,
            codecs: Vec::new(),
            bundler_info: None,
            _context: PhantomData,
        }
    }

    /// `addPacket(PacketType<P> type, StreamCodec<? super B, P> serializer)`.
    ///
    /// Registration order is network id: the `n`-th call owns id `n` (checked
    /// by `ProtocolCodecBuilder`/`IdDispatchCodec` at build). The flow check
    /// fires at build with Java's `IllegalArgumentException` message.
    pub fn add_packet(
        &mut self,
        ty: PacketType,
        serializer: StreamCodec<FriendlyByteBuf, V>,
    ) -> &mut Self {
        self.codecs.push(CodecEntry {
            ty,
            codec: serializer,
        });
        self
    }

    /// `withBundlePacket(PacketType<P> bundlerPacket, Function<..., P> constructor,
    /// D delimiterPacket)`.
    ///
    /// Registers the delimiter packet with a `StreamCodec.unit` codec — so the
    /// delimiter owns network id 0 (Paper's play/clientbound `bundle_delimiter`)
    /// — and records the [`BundlerInfo`]. Java's `constructor` (how a bundle
    /// packet is assembled from its sub-packets) is deferred with the
    /// `BundlePacket` body surface (see [`crate::protocol::bundle`]).
    pub fn with_bundle_packet(
        &mut self,
        bundle_packet_type: PacketType,
        delimiter_packet: V,
    ) -> &mut Self
    where
        V: Packet + Clone + PartialEq + Display,
    {
        let delimiter_packet_type = delimiter_packet.packet_type();
        self.codecs.push(CodecEntry {
            ty: delimiter_packet_type.clone(),
            codec: unit(delimiter_packet),
        });
        self.bundler_info = Some(BundlerInfo::new(bundle_packet_type, delimiter_packet_type));
        self
    }

    /// `buildPacketCodec(contextWrapper, codecs, context)` — the id-dispatch
    /// codec, validated per entry (`ProtocolCodecBuilder.add`). The Java
    /// `contextWrapper` is absorbed (`B = FriendlyByteBuf`).
    fn build_codec(&self) -> StreamCodec<FriendlyByteBuf, V>
    where
        V: Packet,
    {
        let mut codec_builder = ProtocolCodecBuilder::new(self.flow);
        for entry in &self.codecs {
            codec_builder.add(entry.ty.clone(), entry.codec.clone());
        }
        codec_builder.build()
    }

    /// `buildDetails(protocol, flow, codecs)` — the `(PacketType, networkId)`
    /// registration table, network id == `addPacket` index.
    fn build_details(&self) -> ProtocolDetails {
        let packets = self
            .codecs
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.ty.clone(), index as u32))
            .collect();
        ProtocolDetails::new(self.protocol, self.flow, packets)
    }

    /// `buildUnbound(C context)` — freezes the builder into a
    /// [`SimpleUnboundProtocol`]. Java bakes `context` in for bind-time
    /// modifiers; modifiers are deferred (RivetTodo(#126) at module scope), so
    /// the codec is built now and `context` is unused.
    pub fn build_unbound(&mut self, _context: C) -> SimpleUnboundProtocol<V>
    where
        V: Packet,
    {
        let codec = self.build_codec();
        let details = self.build_details();
        let protocol_info =
            ProtocolInfo::new(self.protocol, self.flow, codec, self.bundler_info.clone());
        SimpleUnboundProtocol::new(protocol_info, details)
    }

    /// `buildUnbound()` — freezes the builder into an [`UnboundProtocol`] that
    /// takes the context at `bind`.
    pub fn build_unbound_context(&mut self) -> UnboundProtocol<V, C>
    where
        V: Packet,
    {
        let codec = self.build_codec();
        let details = self.build_details();
        let protocol_info =
            ProtocolInfo::new(self.protocol, self.flow, codec, self.bundler_info.clone());
        UnboundProtocol::new(protocol_info, details)
    }
}

/// `ProtocolInfoBuilder.serverboundProtocol(ConnectionProtocol, Consumer)`.
///
/// Builds a [`SimpleUnboundProtocol`] for the serverbound (client→server)
/// direction, mirroring `HandshakeProtocols.SERVERBOUND_TEMPLATE` and friends.
pub fn serverbound_protocol<V>(
    id: ConnectionProtocol,
    config: impl FnOnce(&mut ProtocolInfoBuilder<V, ()>),
) -> SimpleUnboundProtocol<V>
where
    V: 'static + Packet,
{
    let mut builder = ProtocolInfoBuilder::new(id, PacketFlow::Serverbound);
    config(&mut builder);
    builder.build_unbound(())
}

/// `ProtocolInfoBuilder.clientboundProtocol(...)` — the clientbound
/// (server→client) sibling of [`serverbound_protocol`].
pub fn clientbound_protocol<V>(
    id: ConnectionProtocol,
    config: impl FnOnce(&mut ProtocolInfoBuilder<V, ()>),
) -> SimpleUnboundProtocol<V>
where
    V: 'static + Packet,
{
    let mut builder = ProtocolInfoBuilder::new(id, PacketFlow::Clientbound);
    config(&mut builder);
    builder.build_unbound(())
}

/// `ProtocolInfoBuilder.contextServerboundProtocol(...)` — the context
/// serverbound sibling of [`serverbound_protocol`], returning an
/// [`UnboundProtocol`] (context applied at bind; currently unused, see the
/// module doc).
pub fn context_serverbound_protocol<V, C>(
    id: ConnectionProtocol,
    config: impl FnOnce(&mut ProtocolInfoBuilder<V, C>),
) -> UnboundProtocol<V, C>
where
    V: 'static + Packet,
{
    let mut builder = ProtocolInfoBuilder::new(id, PacketFlow::Serverbound);
    config(&mut builder);
    builder.build_unbound_context()
}

/// `ProtocolInfoBuilder.contextClientboundProtocol(...)` — the context
/// clientbound sibling of [`context_serverbound_protocol`].
pub fn context_clientbound_protocol<V, C>(
    id: ConnectionProtocol,
    config: impl FnOnce(&mut ProtocolInfoBuilder<V, C>),
) -> UnboundProtocol<V, C>
where
    V: 'static + Packet,
{
    let mut builder = ProtocolInfoBuilder::new(id, PacketFlow::Clientbound);
    config(&mut builder);
    builder.build_unbound_context()
}
