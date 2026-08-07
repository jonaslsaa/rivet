//! Port of `net.minecraft.network.protocol.configuration.ClientboundUpdateEnabledFeaturesPacket`
//! (issue #109).
//!
//! Java: `ClientboundUpdateEnabledFeaturesPacket.java` in `working/Paper`. A
//! `Set<Identifier>` of enabled feature flags. The decode ctor reads
//! `input.readCollection(HashSet::new, FriendlyByteBuf::readIdentifier)` — the
//! `HashSet::new` method reference binds to `HashSet(int initialCapacity)`, so
//! a negative wire count panics with Java's `"Illegal initial capacity: {n}"`.
//! The wire count is a varint and the set order on the wire is **not
//! contractually stable** (Java `HashSet` iteration order is hash-dependent);
//! tests compare decoded content, never byte identity, for multi-element sets
//! (the capture-semantics rule in `PORTING.md`).

use crate::codec::byte_buf_codecs::string_utf8;
use crate::codec::{CodecError, StreamCodec, StreamDecoder, of};
use crate::friendly_byte_buf::{FriendlyByteBuf, MAX_STRING_LENGTH};
use crate::protocol::configuration::packet_types::clientbound_update_enabled_features;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_registry::Identifier;

use std::collections::HashSet;

/// `net.minecraft.network.protocol.configuration.ClientboundUpdateEnabledFeaturesPacket` —
/// the record `(Set<Identifier> features)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundUpdateEnabledFeaturesPacket {
    features: HashSet<Identifier>,
}

impl ClientboundUpdateEnabledFeaturesPacket {
    /// `new ClientboundUpdateEnabledFeaturesPacket(Set<Identifier> features)`.
    pub fn new(features: HashSet<Identifier>) -> Self {
        ClientboundUpdateEnabledFeaturesPacket { features }
    }

    /// `ClientboundUpdateEnabledFeaturesPacket.features()`.
    pub fn features(&self) -> &HashSet<Identifier> {
        &self.features
    }

    /// `ClientboundUpdateEnabledFeaturesPacket.STREAM_CODEC` — the varint-count
    /// set of `Identifier.STREAM_CODEC` elements.
    ///
    /// Set order on the wire is arbitrary (Java `HashSet` iteration), so a
    /// decode→encode round-trip is not guaranteed byte-identical — exactly
    /// Java's `HashSet` writeCollection. A malformed element key surfaces as
    /// `Err` (Java `IdentifierException`), not a panic.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundUpdateEnabledFeaturesPacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ClientboundUpdateEnabledFeaturesPacket| {
                // `writeCollection(Set, FriendlyByteBuf::writeIdentifier)` — the
                // count first, then the set in its own (arbitrary) iteration
                // order.
                output.write_var_int(value.features.len() as i32);
                for id in &value.features {
                    output.write_utf(&id.to_string());
                }
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                let count = input.read_var_int();
                // `HashSet(int)` — negative capacity is Java's
                // `IllegalArgumentException("Illegal initial capacity: {n}")`.
                if count < 0 {
                    panic!("Illegal initial capacity: {count}");
                }
                let mut features = HashSet::with_capacity(count as usize);
                for _ in 0..count {
                    // `FriendlyByteBuf.readIdentifier` -> `Identifier.parse`,
                    // surfaced as `Err` (Java `IdentifierException`).
                    let s = string_utf8(MAX_STRING_LENGTH).decode(input)?;
                    let id = Identifier::by_separator_result(&s, ':')
                        .map_err(|e| CodecError::new(e.message().to_string()))?;
                    features.insert(id);
                }
                Ok(ClientboundUpdateEnabledFeaturesPacket { features })
            },
        )
    }
}

impl Packet for ClientboundUpdateEnabledFeaturesPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_update_enabled_features()
    }
}
