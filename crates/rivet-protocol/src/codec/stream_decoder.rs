//! Port of `net.minecraft.network.codec.StreamDecoder`.

use crate::codec::CodecError;

/// `StreamDecoder<I, T>` — `T decode(I input)`.
///
/// Netty's `DecoderException` is a `RuntimeException` that Paper does not catch
/// per-codec: it surfaces at the frame boundary and kicks the connection. Per
/// PORTING.md's checked-exception rule it maps to `Err(CodecError)` here so the
/// frame boundary can decide, matching the `CorruptedFrameException` precedent
/// in `varint21_frame_decoder`. This is the one deliberate divergence from the
/// raw `FriendlyByteBuf` layer, which still panics for the same netty
/// exceptions (see its module docs).
pub trait StreamDecoder<I, T> {
    /// `T decode(I input)` — consumes from `input` and returns the value.
    fn decode(&self, input: &mut I) -> Result<T, CodecError>;
}
