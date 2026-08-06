//! Port of `net.minecraft.network.Varint21LengthFieldPrepender` — `extends
//! MessageToByteEncoder<ByteBuf>`.
//!
//! Java: `Varint21LengthFieldPrepender.java` in `working/Paper` (vanilla 26.2). The
//! Java class is `@Sharable` and stateless, so the encode step is a free function over
//! `bytes::BytesMut` (the scaffold wires only `bytes`; no netty / tokio `Codec`).

use bytes::BytesMut;

/// `Varint21LengthFieldPrepender.MAX_VARINT21_BYTES` (Java `public static final int = 3`).
pub const MAX_VARINT21_BYTES: usize = 3;

/// `EncoderException` — thrown by [`encode_frame`] when the varint header would exceed
/// three bytes. Mirrors the unchecked `io.netty.handler.codec.EncoderException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderException {
    pub message: String,
}

impl EncoderException {
    pub fn new(message: impl Into<String>) -> Self {
        EncoderException {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for EncoderException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EncoderException {}

/// `encode(...)` — frames `payload` as `varint(len) ++ payload`.
///
/// Mirrors the Java `encode` in order: `headerLength = VarInt.getByteSize(bodyLength)`,
/// reject when `headerLength > 3`, then write the varint header and copy the body.
pub fn encode_frame(payload: &[u8]) -> Result<BytesMut, EncoderException> {
    let body_length = payload.len();
    let header_length = get_byte_size(body_length);
    if header_length > MAX_VARINT21_BYTES {
        return Err(EncoderException::new(format!(
            "Packet too large: size {body_length} is over 8"
        )));
    }

    let mut out = BytesMut::with_capacity(header_length + body_length);
    write_varint(&mut out, body_length);
    out.extend_from_slice(payload);
    Ok(out)
}

/// `VarInt.write` — the Paper peel of the one- and two-byte cases, then `writeSlow`.
fn write_varint(out: &mut BytesMut, value: usize) {
    if value & (usize::MAX << 7) == 0 {
        out.extend_from_slice(&[value as u8]);
    } else if value & (usize::MAX << 14) == 0 {
        let s = (((value & 0x7F) | 0x80) << 8 | (value >> 7)) as u16;
        out.extend_from_slice(&s.to_be_bytes());
    } else {
        write_varint_slow(out, value);
    }
}

/// `VarInt.writeSlow`.
fn write_varint_slow(out: &mut BytesMut, mut value: usize) {
    while (value & !0x7F) != 0 {
        out.extend_from_slice(&[((value & 0x7F) | 0x80) as u8]);
        value >>= 7;
    }
    out.extend_from_slice(&[value as u8]);
}

/// `VarInt.getByteSize(int)` — the `VARINT_EXACT_BYTE_LENGTHS[Integer.numberOfLeadingZeros]`
/// table, i.e. `ceil(bit_length(v) / 7)` for `v >= 1` and `1` for `v == 0`.
///
/// Stays local (duplicated in `varint21_frame_decoder.rs`) because
/// `crate::var_int::get_byte_size` is `i32`-scoped while frame lengths are `usize`.
fn get_byte_size(value: usize) -> usize {
    if value == 0 {
        return 1;
    }
    let bits = usize::BITS as usize - value.leading_zeros() as usize;
    bits.div_ceil(7)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn frames_payload_with_varint_length() {
        let out = encode_frame(b"Hello").unwrap();
        assert_eq!(&out[..], b"\x05Hello");
    }

    #[test]
    fn empty_payload_gets_zero_length() {
        let out = encode_frame(b"").unwrap();
        assert_eq!(&out[..], b"\x00");
    }

    #[test]
    fn payload_follows_header_verbatim() {
        let payload = b"Hello, world!";
        let out = encode_frame(payload).unwrap();
        assert_eq!(&out[1..], payload);
    }

    #[test]
    fn varint_header_boundaries() {
        // 1-byte headers: lengths 0..=127.
        let out = encode_frame(&[0u8; 1]).unwrap();
        assert_eq!(&out[..1], b"\x01");
        let out = encode_frame(&[0u8; 127]).unwrap();
        assert_eq!(&out[..1], b"\x7F");

        // 2-byte headers: lengths 128..=16383.
        let out = encode_frame(&[0u8; 128]).unwrap();
        assert_eq!(&out[..2], b"\x80\x01");
        let out = encode_frame(&[0u8; 16383]).unwrap();
        assert_eq!(&out[..2], b"\xFF\x7F");

        // 3-byte headers: lengths 16384..=2097151.
        let out = encode_frame(&[0u8; 16384]).unwrap();
        assert_eq!(&out[..3], b"\x80\x80\x01");
        let out = encode_frame(&[0u8; 2097151]).unwrap();
        assert_eq!(&out[..3], b"\xFF\xFF\x7F");
    }

    #[test]
    fn frame_header_is_varint_of_body_length() {
        // The header of a 1000-byte payload must equal the varint encoding of 1000.
        let out = encode_frame(&vec![0u8; 1000]).unwrap();
        let mut expected = BytesMut::new();
        write_varint(&mut expected, 1000);
        assert_eq!(&out[..expected.len()], &expected[..]);
    }

    #[test]
    fn too_large_payload_is_rejected() {
        let err = encode_frame(&vec![0u8; 2097152]).unwrap_err();
        assert_eq!(err.message, "Packet too large: size 2097152 is over 8");
    }

    #[test]
    fn round_trip_with_decoder() {
        let payload = vec![0x5Au8; 1000];
        let mut frame = encode_frame(&payload).unwrap();
        let decoder = crate::varint21_frame_decoder::Varint21FrameDecoder::new(None);
        assert_eq!(
            decoder.decode(&mut frame).unwrap().unwrap(),
            Bytes::from(payload)
        );
        assert!(frame.is_empty());
    }
}
