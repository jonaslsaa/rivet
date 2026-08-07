//! Java hashing algorithms used in game logic — `rivet-util::java_hash`
//! (PORTING.md).
//!
//! Faithful ports of:
//! - `String.hashCode` (used pervasively for seeds, thread names, and the
//!   `WorldOptions` seed-string path)
//! - `Mth.murmurHash3Mixer` (`net.minecraft.util.Mth`)
//! - `Mth.getSeed(x, y, z)` (`net.minecraft.util.Mth`) — the positional seed
//!   hash consumed by `LegacyRandomSource`, `XoroshiroRandomSource`, and block
//!   random-tick seeding.
//!
//! All three are exact algorithms; golden values below were produced by
//! running the reference implementation on OpenJDK 25 (the Java runtime in
//! this environment), not invented.
//!
//! RivetTodo(#205): `net.minecraft.util.HashOps` — the only other hash-named
//! type in the package — is not ported here: it is a ~390-line
//! `DynamicOps<HashCode>` DFU serialization adapter (Guava `Hashing`/`CRC32C`,
//! `DataResult`, `RecordBuilder`, `ListBuilder`), not a hash algorithm; it is
//! a plain omission with no consumer forcing it.

/// `String.hashCode()` — the JDK algorithm over UTF-16 code units.
///
/// Java computes `h = 31 * h + char` for each UTF-16 code unit (`char`, an
/// unsigned 16-bit value). Supplementary characters (astral planes) contribute
/// both surrogate code units, so the hash iterates the UTF-16 encoding, not
/// Unicode scalar values — `s.encode_utf16()` in Rust yields exactly those
/// code units.
pub fn string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(unit as i32);
    }
    h
}

/// `Mth.murmurHash3Mixer(int hash)` — the 32-bit murmur3 finalizer used as an
/// identity-hash mixer (e.g. `CrudeIncrementalIntIdentityHashBiMap.hash`).
///
/// Java `>>>` is a logical shift (performed on the `u32` view); `*=` wraps.
pub fn murmur_hash3_mixer(mut hash: i32) -> i32 {
    hash ^= ((hash as u32) >> 16) as i32;
    hash = hash.wrapping_mul(-2048144789);
    hash ^= ((hash as u32) >> 13) as i32;
    hash = hash.wrapping_mul(-1028477387);
    hash ^ ((hash as u32) >> 16) as i32
}

/// `Mth.getSeed(int x, int y, int z)` — the deprecated positional seed hash.
///
/// Java expression `long seed = x * 3129871 ^ z * 116129781L ^ y;`:
/// - `x * 3129871` is **int** arithmetic (32-bit wrapping), sign-extended
///   before the XOR with the long operand `z * 116129781L`;
/// - `z * 116129781L` is long arithmetic (`116129781L` promotes `z`).
///
/// The trailing `>> 16` is Java's arithmetic (sign-preserving) shift on a
/// `long`, so `seed >> 16` in Rust on `i64` is faithful.
///
/// The `@Deprecated` overload `getSeed(Vec3i)` (delegates to this one) awaits
/// `net.minecraft.core.Vec3i` (rivet-core).
pub fn get_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut seed =
        (x.wrapping_mul(3129871)) as i64 ^ ((z as i64).wrapping_mul(116129781)) ^ (y as i64);
    seed = seed
        .wrapping_mul(seed)
        .wrapping_mul(42317861)
        .wrapping_add(seed.wrapping_mul(11));
    seed >> 16
}

#[cfg(test)]
mod tests {
    use super::{get_seed, murmur_hash3_mixer, string_hash};

    /// Golden values produced by running the reference algorithms on OpenJDK
    /// 25 (see the class comment in the Java `Mth.java` / `String.hashCode`).
    #[test]
    fn string_hash_golden() {
        assert_eq!(string_hash("hello"), 99162322);
        assert_eq!(string_hash(""), 0);
        assert_eq!(string_hash("minecraft"), 695073197);
        assert_eq!(string_hash("minecraft:overworld"), 1104210353);
        assert_eq!(string_hash("minecraft:stone"), -1133948840);
        assert_eq!(string_hash("minecraft:air"), 1768632829);
        assert_eq!(string_hash("minecraft:zombie"), -596960109);
        assert_eq!(string_hash("overworld"), -745159874);
        assert_eq!(string_hash("the_nether"), 1272296422);
        assert_eq!(string_hash("The End"), 312628332);
        assert_eq!(string_hash("level.dat"), -1657362899);
        assert_eq!(string_hash("random.seed"), -744840004);
    }

    /// Supplementary-plane characters hash the two UTF-16 surrogate code units,
    /// not the scalar value — `encode_utf16()` must match Java's UTF-16 `char`
    /// iteration exactly (PORTING.md UTF-16 drift checklist).
    #[test]
    fn string_hash_utf16_surrogates() {
        assert_eq!(string_hash("😀"), 1772899); // U+1F600
        assert_eq!(string_hash("🚀"), 1773027); // U+1F680
        assert_eq!(string_hash("𝕏"), 1772474); // U+1D54F
        assert_eq!(string_hash("ab😀c"), 147461023);
    }

    /// Short strings exercise the 31-multiplier arithmetic directly
    /// (`"aa"` = 31*97+97 = 3104, `"ab"` = 31*97+98 = 3105, `"a b"` = 94307).
    #[test]
    fn string_hash_short() {
        assert_eq!(string_hash("aa"), 3104);
        assert_eq!(string_hash("ab"), 3105);
        assert_eq!(string_hash("aaa"), 96321);
        assert_eq!(string_hash("a b"), 94307);
    }

    #[test]
    fn murmur_hash3_mixer_golden() {
        assert_eq!(murmur_hash3_mixer(0), 0);
        assert_eq!(murmur_hash3_mixer(1), 1364076727);
        assert_eq!(murmur_hash3_mixer(-1), -2114883783);
        assert_eq!(murmur_hash3_mixer(42), 142593372);
        assert_eq!(murmur_hash3_mixer(123456789), -1168058214);
        assert_eq!(murmur_hash3_mixer(i32::MAX), -104067416);
        assert_eq!(murmur_hash3_mixer(i32::MIN), 1832674720);
        assert_eq!(murmur_hash3_mixer(99162322), 837524112);
    }

    #[test]
    fn get_seed_golden() {
        assert_eq!(get_seed(0, 0, 0), 0);
        assert_eq!(get_seed(0, 0, 1), -20769809646864);
        assert_eq!(get_seed(1, 0, 0), 133076631897947);
        assert_eq!(get_seed(0, 1, 0), 645);
        assert_eq!(get_seed(1, 2, 3), -33674130277896);
        assert_eq!(get_seed(-1, -1, -1), 60311958933234);
        assert_eq!(get_seed(15, 64, 15), -37618236202976);
        assert_eq!(get_seed(256, 100, -256), 76943884593746);
        assert_eq!(get_seed(12345, -67890, 999999), -80994335472508);
        // int-overflow and boundary extremes (values from the Java reference run).
        assert_eq!(get_seed(-1, 1, -1), 60311958971344);
        assert_eq!(get_seed(2147483647, -2147483648, 0), 133076631896896);
        assert_eq!(get_seed(7, 7, 7), -35564658949879);
        assert_eq!(get_seed(12345, -9876, 55555), 58516538991611);
    }
}
