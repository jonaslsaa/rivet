//! Port of `net.minecraft.network.RegistryFriendlyByteBuf` (MC 26.2).
//!
//! Java: `RegistryFriendlyByteBuf.java` in `working/Paper` (vanilla 26.2), a
//! thin `FriendlyByteBuf` subclass carrying a `RegistryAccess`:
//!
//! ```java
//! public class RegistryFriendlyByteBuf extends FriendlyByteBuf {
//!     private final RegistryAccess registryAccess;
//!     public RegistryAccess registryAccess() { return this.registryAccess; }
//! }
//! ```
//!
//! The registry-aware `ByteBufCodecs` methods (`registry`/`holderRegistry`/
//! `holder`/`holderSet`, #126 phase G) are `StreamCodec<RegistryFriendlyByteBuf,
//! ...>` and resolve their registry through `input.registryAccess()
//! .lookupOrThrow(..)` — see [`crate::codec::registry_byte_buf_codecs`].
//!
//! Java inherits the whole `FriendlyByteBuf` method set onto this subclass; the
//! Rust port models that inheritance with the delegate methods below (the raw
//! `FriendlyByteBuf` surface the codecs need), plus the registry-aware
//! conveniences (`readIdentifier`/`readResourceKey`/`readGlobalPos`, …) whose
//! registry key comes from this buffer. The plain `FriendlyByteBuf` deliberately
//! carries no `RegistryAccess`, so those conveniences land here.

use bytes::BytesMut;
use rivet_registry::Identifier;
use rivet_registry::RegistryAccess;
use rivet_registry::ResourceKey;
use rivet_registry::core::{BlockPos, GlobalPos};
use rivet_registry::registries;
use rivet_registry::registry::RegistryKey;

use crate::friendly_byte_buf::FriendlyByteBuf;

/// `net.minecraft.network.RegistryFriendlyByteBuf` — a `FriendlyByteBuf` that
/// carries the `RegistryAccess` the registry-aware codecs resolve through.
#[derive(Debug, Clone)]
pub struct RegistryFriendlyByteBuf {
    inner: FriendlyByteBuf,
    registry_access: RegistryAccess,
}

impl RegistryFriendlyByteBuf {
    /// `new RegistryFriendlyByteBuf(ByteBuf, RegistryAccess)`.
    pub fn new(inner: BytesMut, registry_access: RegistryAccess) -> Self {
        RegistryFriendlyByteBuf {
            inner: FriendlyByteBuf::new(inner),
            registry_access,
        }
    }

    /// `RegistryFriendlyByteBuf.registryAccess()`.
    pub fn registry_access(&self) -> &RegistryAccess {
        &self.registry_access
    }

    /// The underlying `BytesMut`.
    pub fn into_inner(self) -> BytesMut {
        self.inner.into_inner()
    }

    /// The inner `FriendlyByteBuf` (Java's inherited `this`).
    pub fn inner(&self) -> &FriendlyByteBuf {
        &self.inner
    }

    /// The inner `FriendlyByteBuf`, mutable (Java's inherited `this`).
    pub fn inner_mut(&mut self) -> &mut FriendlyByteBuf {
        &mut self.inner
    }

    /// The readable bytes (netty `ByteBuf.readableBytes()`).
    pub fn readable_bytes(&self) -> usize {
        self.inner.readable_bytes()
    }

    /// The unread prefix, for direct inspection.
    pub fn as_slice(&self) -> &[u8] {
        self.inner.as_slice()
    }

    // ---- inherited FriendlyByteBuf surface (delegates) --------------------

    /// `readUtf()` — bounded at `MAX_STRING_LENGTH`.
    pub fn read_utf(&mut self) -> String {
        self.inner.read_utf()
    }

    /// `writeUtf(String)` — bounded at `MAX_STRING_LENGTH`.
    pub fn write_utf(&mut self, value: &str) {
        self.inner.write_utf(value);
    }

    /// `readVarInt()`.
    pub fn read_var_int(&mut self) -> i32 {
        self.inner.read_var_int()
    }

    /// `writeVarInt(int)`.
    pub fn write_var_int(&mut self, value: i32) {
        self.inner.write_var_int(value);
    }

    /// `readLong()`.
    pub fn read_long(&mut self) -> i64 {
        self.inner.read_long()
    }

    /// `writeLong(long)`.
    pub fn write_long(&mut self, value: i64) {
        self.inner.write_long(value);
    }

    // ---- registry-aware conveniences -------------------------------------
    // Java inherits these onto `FriendlyByteBuf` (`readIdentifier`/`readResourceKey`/
    // `readBlockPos`/`readGlobalPos`); the `RegistryAccess` the consumers
    // (CommonPlayerSpawnInfo, configuration registry sync) run over is this
    // buffer, so they land here.

    /// `FriendlyByteBuf.readIdentifier()` — `Identifier.parse(readUtf())`.
    pub fn read_identifier(&mut self) -> Identifier {
        Identifier::parse(&self.read_utf())
    }

    /// `FriendlyByteBuf.writeIdentifier(Identifier)`.
    pub fn write_identifier(&mut self, identifier: &Identifier) {
        self.write_utf(&identifier.to_string());
    }

    /// `FriendlyByteBuf.readResourceKey(ResourceKey<? extends Registry<T>>)`.
    pub fn read_resource_key<T: 'static>(&mut self, registry: &RegistryKey<T>) -> ResourceKey<T> {
        ResourceKey::create(registry, self.read_identifier())
    }

    /// `FriendlyByteBuf.writeResourceKey(ResourceKey<?>)`.
    pub fn write_resource_key<T>(&mut self, key: &ResourceKey<T>) {
        self.write_identifier(key.identifier());
    }

    /// `FriendlyByteBuf.readBlockPos()`.
    pub fn read_block_pos(&mut self) -> BlockPos {
        BlockPos::of_long(self.read_long())
    }

    /// `FriendlyByteBuf.writeBlockPos(BlockPos)`.
    pub fn write_block_pos(&mut self, pos: &BlockPos) {
        self.write_long(pos.as_long());
    }

    /// `FriendlyByteBuf.readGlobalPos()` — a `ResourceKey<Level>` then a
    /// `BlockPos`, the `Registries.DIMENSION` registry key.
    pub fn read_global_pos(&mut self) -> GlobalPos {
        GlobalPos::of(
            self.read_resource_key(&*registries::DIMENSION),
            self.read_block_pos(),
        )
    }

    /// `FriendlyByteBuf.writeGlobalPos(GlobalPos)`.
    pub fn write_global_pos(&mut self, pos: &GlobalPos) {
        self.write_resource_key(pos.dimension());
        self.write_block_pos(&pos.pos());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    /// A fresh buffer with an empty access — the convenience methods only touch
    /// the raw bytes (their registry keys are passed in / statically known), so
    /// no `RegistryAccess` content is needed.
    fn empty_buffer() -> RegistryFriendlyByteBuf {
        RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty())
    }

    #[test]
    fn read_identifier_wire_form_and_round_trip() {
        let mut buf = empty_buffer();
        buf.write_identifier(&Identifier::parse("minecraft:stone"));
        // Wire form: `STRING_UTF8` — varint byte-length then the UTF-8 bytes.
        assert_eq!(buf.as_slice(), b"\x0fminecraft:stone");
        assert_eq!(buf.read_identifier(), Identifier::parse("minecraft:stone"));
    }

    #[test]
    fn write_read_resource_key_round_trips() {
        let mut buf = empty_buffer();
        let key = ResourceKey::create(
            &*registries::DIMENSION,
            Identifier::with_default_namespace("overworld"),
        );
        buf.write_resource_key(&key);
        // Wire form is just the location identifier (the registry key is not on
        // the wire).
        assert_eq!(buf.as_slice(), b"\x13minecraft:overworld");
        assert_eq!(buf.read_resource_key(&*registries::DIMENSION), key);
    }

    #[test]
    fn read_block_pos_wire_form_and_round_trip() {
        let mut buf = empty_buffer();
        let pos = BlockPos::new(1, -2, 3);
        buf.write_block_pos(&pos);
        // Wire form: `BlockPos.asLong()` big-endian.
        assert_eq!(buf.as_slice(), &pos.as_long().to_be_bytes());
        assert_eq!(buf.read_block_pos(), pos);
    }

    #[test]
    fn write_read_global_pos_round_trips_with_wire_form() {
        let mut buf = empty_buffer();
        let pos = GlobalPos::of(
            ResourceKey::create(
                &*registries::DIMENSION,
                Identifier::with_default_namespace("overworld"),
            ),
            BlockPos::new(10, 64, -20),
        );
        buf.write_global_pos(&pos);
        // Wire: the dimension resource key (identifier string) then the packed
        // long. "minecraft:overworld" is 19 chars.
        let mut expected = vec![19];
        expected.extend_from_slice(b"minecraft:overworld");
        expected.extend_from_slice(&BlockPos::new(10, 64, -20).as_long().to_be_bytes());
        assert_eq!(buf.as_slice(), &expected);
        assert_eq!(buf.read_global_pos(), pos);
    }
}
