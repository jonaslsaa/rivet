//! `net.minecraft.core.UUIDUtil` — the offline-player UUID (issue #198).
//!
//! Java source: `working/Paper/.../net/minecraft/core/UUIDUtil.java`. Only the
//! offline identity slice is ported here; the codec/`StreamCodec` side lives in
//! `rivet-protocol` (the ownership rule: pure value types + `UUIDUtil` stay in
//! `rivet-registry::core`, only their `StreamCodec` impls cross to
//! `rivet-protocol`).
//!
//! Ported in `rivet-protocol` (per the ownership rule above):
//! `UUIDUtil.STREAM_CODEC` lives as `FriendlyByteBuf::read_uuid`/`write_uuid`
//! (the same wire form — two big-endian longs).
//!
//! Deliberately deferred (blocked by later work; no declarations emitted):
//! RivetTodo(#99): the string codecs (`STRING_CODEC`/`AUTHLIB_CODEC`/`LENIENT_CODEC`)
//! and `createOfflineProfile` — a `GameProfile` convenience for #99's offline
//! login construction. RivetTodo(#126): `uuidToByteArray`
//! (`ByteBuffer.wrap(...).order(BIG_ENDIAN)`) — a byte-order primitive with no
//! consumer yet — defers with the protocol StreamCodec surface. The codec slice (#373) adds
//! `CODEC` — `Codec.INT_STREAM.comapFlatMap(Util.fixedSize(list, 4) …
//! uuidFromIntArray, uuid -> IntStream.of(uuidToIntArray(uuid)))` — plus the
//! exact `uuidFromIntArray`/`uuidToIntArray` int-array conversions it builds on.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::mth::Uuid;
use rivet_util::util::fixed_size_i32;
use std::sync::Arc;

/// `UUIDUtil.createOfflinePlayerUUID(String playerName)`.
///
/// Java:
/// `UUID.nameUUIDFromBytes(("OfflinePlayer:" + playerName).getBytes(UTF_8))`.
/// `UUID.nameUUIDFromBytes` is the MD5 of the UTF-8 bytes with the RFC 4122
/// version 3 (name-based) / variant bits set, then read big-endian as
/// `(mostSignificantBits, leastSignificantBits)`:
///
/// ```text
/// md5[6] = (md5[6] & 0x0F) | 0x30;   // version 3
/// md5[8] = (md5[8] & 0x3F) | 0x80;   // RFC 4122 variant
/// ```
///
/// The name is the **UTF-8** encoding of `"OfflinePlayer:" + playerName`
/// (Java encodes the UTF-16 string to UTF-8 before hashing — no UTF-16 units
/// feed the digest). Empty names are legal.
pub fn create_offline_player_uuid(player_name: &str) -> Uuid {
    let digest = md5::compute(format!("OfflinePlayer:{player_name}").as_bytes());
    let mut bytes = digest.0;
    bytes[6] = (bytes[6] & 0x0F) | 0x30;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let mut most: i64 = 0;
    let mut least: i64 = 0;
    for &b in &bytes[0..8] {
        most = (most << 8) | (b as i64 & 0xFF);
    }
    for &b in &bytes[8..16] {
        least = (least << 8) | (b as i64 & 0xFF);
    }
    Uuid { most, least }
}

/// `UUIDUtil.uuidFromIntArray(int[] intArray)` — `new UUID((long)intArray[0] <<
/// 32 | intArray[1] & 4294967295L, (long)intArray[2] << 32 | intArray[3] &
/// 4294967295L)`.
///
/// Java's `(long)x << 32 | y & 0xFFFFFFFFL`: the low int is masked to its
/// unsigned 32 bits before the OR, so `most` for `[0, -1, 0, 0]` is
/// `0xFFFFFFFFL` — the POSITIVE long `4294967295` in Java (bit 63 is clear, so
/// the `i64` here matches as a positive value), never `-1`; `-1` appears only
/// when the upper int's sign bit lands at bit 63 (e.g. `[-1, -1, ...]`). The
/// upper int is cast to `i64` and shifted — `(long)0xFFFFFFFF << 32`
/// sign-extends to `-1L` then wraps to `0xFFFFFFFF00000000`, the negative long
/// `-4294967296` (bit 63 set), matching Rust's wrapping `i64 << 32`; only an
/// upper int whose bit 31 is clear keeps the high bit clear in the result.
/// Mirrored by `uuid_to_int_array`.
///
/// Java indexes `intArray[0..3]` unchecked (no length validation — `CODEC`
/// feeds it through `Util.fixedSize`); the port takes a `&[i32; 4]` so a caller
/// cannot pass a short array.
pub fn uuid_from_int_array(int_array: &[i32; 4]) -> Uuid {
    Uuid {
        most: (int_array[0] as i64) << 32 | (int_array[1] as u32 as i64),
        least: (int_array[2] as i64) << 32 | (int_array[3] as u32 as i64),
    }
}

/// `UUIDUtil.uuidToIntArray(UUID)` — `leastMostToIntArray(most, least)`:
/// `{(int)(most >> 32), (int)most, (int)(least >> 32), (int)least}` — the four
/// 32-bit halves, most-significant first.
pub fn uuid_to_int_array(uuid: Uuid) -> [i32; 4] {
    let most = uuid.most;
    let least = uuid.least;
    [
        (most >> 32) as i32,
        most as i32,
        (least >> 32) as i32,
        least as i32,
    ]
}

/// `UUIDUtil.CODEC` — `Codec.INT_STREAM.comapFlatMap(list ->
/// Util.fixedSize(list, 4).map(UUIDUtil::uuidFromIntArray), uuid ->
/// IntStream.of(uuidToIntArray(uuid)))`.
///
/// `Codec.INT_STREAM` is `Codec<IntStream>`, so Java resolves
/// `Util.fixedSize(list, 4)` to the **`IntStream`** overload
/// (`fixedSize(IntStream, int)`) — same shape as `BlockPos`/`Vec3i` `CODEC`s
/// with size 3. The port's `fixed_size_i32` is that overload: a wrong-length
/// input errors with `"Input is not a list of 4 ints"` and, when longer than 4,
/// carries the first 4 ints as the partial. That partial is then mapped by
/// `uuid_from_int_array` into a partial `Uuid` on the same error (the DFU
/// `flatMap` keeps an error-with-partial as an error). The encode side (`comap`)
/// produces the 4-int list `[most>>32, most, least>>32, least]` via
/// `int_stream_codec`'s `create_int_list`. Ops-generic in the port, hence the
/// `uuid_codec::<Ops>()` factory.
pub fn uuid_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Uuid, Ops>> {
    codec::comap_flat_map::<Vec<i32>, Uuid, Ops>(
        codec::int_stream_codec::<Ops>(),
        Arc::new(|list: &Vec<i32>| {
            fixed_size_i32(list, 4).map(|fs| uuid_from_int_array(&[fs[0], fs[1], fs[2], fs[3]]))
        }),
        Arc::new(|uuid: &Uuid| uuid_to_int_array(*uuid).to_vec()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;

    fn uuid(most: i64, least: i64) -> Uuid {
        Uuid { most, least }
    }

    /// `Uuid` -> the dashed lowercase hex form `UUID.toString()` produces.
    fn to_dashed(u: Uuid) -> String {
        let hex = format!("{:016x}{:016x}", u.most as u64, u.least as u64);
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }

    #[test]
    fn golden_vectors_match_jdk_name_uuid_from_bytes() {
        // Verified against the JDK `UUID.nameUUIDFromBytes` (version 3, RFC
        // 4122 variant) on the pinned local 26.2 toolchain. `""` and multibyte
        // names are legal — the digest input is the UTF-8 bytes.
        for (name, expected) in [
            ("Steve", "5627dd98-e6be-3c21-b8a8-e92344183641"),
            ("Notch", "b50ad385-829d-3141-a216-7e7d7539ba7f"),
            ("jonass", "af5a52f9-6eef-3a52-9668-4bb9f65bc5f8"),
            ("Herobrine", "25966168-dc9c-360c-8f32-ed022bfa1070"),
            ("", "fc5bc365-aedf-30a8-8b89-04e462e29bde"),
            ("Player_123", "592ac8aa-a392-3300-916e-00427a650c3b"),
            ("テスト", "74b382bc-b00a-31bd-a0eb-fb238c2b96ec"),
            ("😀x", "69321201-8378-3a9d-a409-e56482412fc3"),
        ] {
            assert_eq!(
                to_dashed(create_offline_player_uuid(name)),
                expected,
                "name = {name:?}"
            );
        }
    }

    #[test]
    fn rivet_probe_uuid_matches_the_captured_offline_profile() {
        // The pinned capture fixture's login_finished playerUUID (issue #198 /
        // #153 provenance): `UUID.nameUUIDFromBytes("OfflinePlayer:RivetProbe")`.
        // The least half has the high bit set; read as the signed i64.
        assert_eq!(
            create_offline_player_uuid("RivetProbe"),
            uuid(0x0a9f_fa92_a706_3e6f, 0x900c_f12f_869d_37eau64 as i64)
        );
    }

    #[test]
    fn version_and_variant_bits() {
        // `UUID.nameUUIDFromBytes` sets version 3 at the high nibble of md5[6]
        // (`md5[6] = (md5[6] & 0x0F) | 0x30`), i.e. bits 12..16 of `most`, and
        // the RFC 4122 variant (`10`) at the top two bits of md5[8] (the top
        // two bits of `least`).
        let u = create_offline_player_uuid("Steve");
        let version = ((u.most as u64) >> 12) & 0xF;
        assert_eq!(version, 3);
        let variant = (u.least as u64 >> 62) & 0b11;
        assert_eq!(variant, 0b10);
    }

    #[test]
    fn int_array_conversions_match_java_bit_patterns() {
        // `uuidFromIntArray` — `(long)a0 << 32 | a1 & 0xFFFFFFFFL` per half.
        // The low int is masked to its unsigned 32 bits before the OR: `[0,
        // -1, 0, 0]` -> `most == 0xFFFFFFFFL == 4294967295` — a POSITIVE long
        // in Java (the sign bit is not set until bit 63), and the same positive
        // `i64` here. The high int's sign bit lands at bit 63 only when the
        // upper int itself is negative.
        assert_eq!(uuid_from_int_array(&[0, -1, 0, 0]), uuid(0xFFFF_FFFF, 0));
        assert_eq!(
            uuid_from_int_array(&[0x7fff_ffff, -1, 0, 0]),
            uuid(0x7fff_ffff_ffff_ffff, 0)
        );
        // Both halves negative -> every bit set -> -1 as the signed i64.
        assert_eq!(uuid_from_int_array(&[-1, -1, -1, -1]), uuid(-1, -1));
        // The offline "RivetProbe" UUID's halves split into the exact four
        // 32-bit words `uuidToIntArray` recovers.
        let u = create_offline_player_uuid("RivetProbe");
        assert_eq!(
            uuid_to_int_array(u),
            [
                (0x0a9f_fa92u32 as i32),
                (0xa706_3e6fu32 as i32),
                (0x900c_f12fu32 as i32),
                (0x869d_37eau32 as i32),
            ]
        );
    }

    #[test]
    fn int_array_conversions_round_trip() {
        for u in [
            uuid(0, 0),
            uuid(-1, -1),
            uuid(0x0123_4567_89ab_cdef, -0x0123_4567_89ab_cdef),
            create_offline_player_uuid("Steve"),
        ] {
            assert_eq!(uuid_from_int_array(&uuid_to_int_array(u)), u);
        }
        // The 4-int -> Uuid -> 4-int identity (each half splits back exactly).
        for arr in [[0, 0, 0, 0], [-1, -1, -1, -1], [i32::MIN, i32::MAX, 0, 1]] {
            assert_eq!(uuid_to_int_array(uuid_from_int_array(&arr)), arr);
        }
    }

    #[test]
    fn codec_roundtrips_all_values() {
        let ops = JsonOps::INSTANCE;
        let codec = uuid_codec::<JsonOps>();
        for u in [
            uuid(0, 0),
            uuid(-1, -1),
            uuid(0x0123_4567_89ab_cdef, -0x0123_4567_89ab_cdef),
            create_offline_player_uuid("Steve"),
        ] {
            // Encode to the 4-int list `[most>>32, most, least>>32, least]`.
            let encoded = codec.encode_start(&ops, &u).get_or_throw("encode").clone();
            let arr = uuid_to_int_array(u);
            assert_eq!(encoded, ops.create_int_list(arr.to_vec()));
            // Decode back.
            let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
            assert_eq!(decoded.0, u);
        }
    }

    #[test]
    fn codec_rejects_wrong_length_with_fixed_size_message() {
        let ops = JsonOps::INSTANCE;
        let codec = uuid_codec::<JsonOps>();
        for len in [0usize, 1, 2, 3, 5, 8] {
            let input = ops.create_int_list((0..len as i32).collect());
            let result = codec.decode(&ops, &input);
            assert!(result.result().is_none(), "length {len} should fail");
            // `Codec.INT_STREAM` is `Codec<IntStream>`, so Java resolves
            // `Util.fixedSize(list, 4)` to the IntStream overload (as
            // `BlockPos`/`Vec3i` `CODEC`s do) — the message is "ints", not the
            // List overload's "elements".
            assert_eq!(
                result.error_ref().map(|e| e.message().to_string()),
                Some("Input is not a list of 4 ints".to_string())
            );
        }
    }

    #[test]
    fn codec_long_input_keeps_partial_error_ordering() {
        // A 5-int input: `Util.fixedSize(IntStream, 4)` errors with the first 4
        // ints as the partial, and the `flatMap` continuation maps that partial
        // through `uuidFromIntArray` — so the error carries a partial `Uuid`,
        // not the raw int list, exactly as Java's
        // `Codec.INT_STREAM.comapFlatMap(Util.fixedSize(...))` does.
        let ops = JsonOps::INSTANCE;
        let codec = uuid_codec::<JsonOps>();
        let input = ops.create_int_list(vec![1, 2, 3, 4, 5]);
        let result = codec.decode(&ops, &input);
        assert!(result.result().is_none());
        assert_eq!(
            result.error_ref().map(|e| e.message().to_string()),
            Some("Input is not a list of 4 ints".to_string())
        );
        assert_eq!(
            result
                .error_ref()
                .and_then(|e| e.partial().clone())
                .map(|p| p.0),
            Some(uuid_from_int_array(&[1, 2, 3, 4]))
        );
    }
}
