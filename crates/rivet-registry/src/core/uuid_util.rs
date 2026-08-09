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
//! Deliberately deferred (blocked by later work; no declarations emitted): the
//! DFU `CODEC`s (`Codec.INT_STREAM` / string codecs, need
//! `rivet-serialization`), `uuidFromIntArray`/`uuidToIntArray`/`uuidToByteArray`,
//! and `createOfflineProfile` (a `GameProfile` convenience — #99's offline login
//! construction, not this codec slice).

use rivet_util::uuid::Uuid;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
