//! Port of `net.minecraft.network.codec.StreamEncoder`.

use crate::codec::CodecError;

/// `StreamEncoder<O, T>` — `void encode(O output, T value)`.
///
/// As with [`crate::codec::StreamDecoder`], netty's `EncoderException` maps to
/// `Err(CodecError)` at this boundary.
pub trait StreamEncoder<O, T> {
    /// `void encode(O output, T value)` — appends `value` to `output`.
    fn encode(&self, output: &mut O, value: &T) -> Result<(), CodecError>;
}
