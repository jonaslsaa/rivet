//! Port of `net.minecraft.network.codec.StreamMemberEncoder`.

use crate::codec::CodecError;

/// `StreamMemberEncoder<O, T>` — `void encode(T value, O output)`, the
/// member-first argument order used by `StreamCodec.ofMember`.
///
/// `Packet.codec(writer, reader)` is exactly `StreamCodec.ofMember(writer,
/// reader)` (`working/Paper/.../network/protocol/Packet.java:35-36`), so this
/// trait captures the writer's `(value, output)` order without needing to be
/// stored: the erased [`crate::codec::StreamCodec`] value implements only the
/// standard `(output, value)` halves, matching Java's `StreamCodec` interface
/// (which is `StreamEncoder`/`StreamDecoder`, never `StreamMemberEncoder`).
pub trait StreamMemberEncoder<O, T> {
    /// `void encode(T value, O output)`.
    fn encode(&self, value: &T, output: &mut O) -> Result<(), CodecError>;
}

/// A `(value, output)` closure is a `StreamMemberEncoder` — Java's
/// `@FunctionalInterface`, which any lambda implements.
impl<O: 'static, T: 'static, F> StreamMemberEncoder<O, T> for F
where
    F: Fn(&T, &mut O) -> Result<(), CodecError> + Send + Sync + 'static,
{
    fn encode(&self, value: &T, output: &mut O) -> Result<(), CodecError> {
        self(value, output)
    }
}
