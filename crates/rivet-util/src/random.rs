//! `net.minecraft.world.level.levelgen` + `net.minecraft.util.RandomSource` RNG
//! port — the Java-parity random surface for rivet-util (CRATES.md).
//!
//! Ported faithfully:
//! - `net.minecraft.util.RandomSource` (the `RandomSource` trait, incl. the
//!   `triangle`/`nextIntBetweenInclusive`/`consumeCount`/`nextInt(origin,
//!   bound)` defaults and the `create*` static factories)
//! - `net.minecraft.world.level.levelgen.BitRandomSource` (the `next(bits)`
//!   LCG-layer trait with the `nextInt`/`nextLong`/`nextFloat`/`nextDouble`
//!   defaults)
//! - `net.minecraft.world.level.levelgen.LegacyRandomSource` (java.util.Random
//!   48-bit LCG, bit-exact) + `LegacyPositionalRandomFactory`
//! - `net.minecraft.world.level.levelgen.XoroshiroRandomSource`
//!   (Xoroshiro128++) + `XoroshiroPositionalRandomFactory`
//! - `net.minecraft.world.level.levelgen.RandomSupport` (Stafford-13 mixing,
//!   md5 `seedFromHashOf`, `generateUniqueSeed`, `Seed128bit`)
//! - `net.minecraft.world.level.levelgen.Xoroshiro128PlusPlus`
//! - `net.minecraft.world.level.levelgen.MarsagliaPolarGaussian` (the stored
//!   nextGaussian quirk)
//! - `net.minecraft.world.level.levelgen.SingleThreadedRandomSource` and
//!   `net.minecraft.world.level.levelgen.ThreadSafeLegacyRandomSource`
//!
//! Precision notes (PORTING.md drift checklist):
//! - Java `long * float` (in `nextDouble`) promotes both operands to `float`,
//!   computes in f32, then widens the result to double. We replicate that with
//!   `(n as f32) * DOUBLE_MULTIPLIER as f32` then `as f64` — NOT a straight f64
//!   multiply. `DOUBLE_MULTIPLIER` is the Java `double` constant
//!   `1.110223E-16F` (float literal widened), so casting it back to `f32` is
//!   lossless.
//! - All int/long arithmetic wraps exactly as in Java (`wrapping_*`).
//! - Java `>>>` is a logical shift on the unsigned view.
//!
//! Threading notes (D5): `LegacyRandomSource` and
//! `SingleThreadedRandomSource` are tick-thread-confined game state, so the
//! Java `AtomicLong`/`ThreadingDetector` machinery is dropped — the `seed` is a
//! plain field. `ThreadSafeLegacyRandomSource` is *for* cross-thread use and
//! keeps an `AtomicI64`.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::java_hash::{get_seed, string_hash as java_string_hash};

// ---------------------------------------------------------------------------
// net.minecraft.util.RandomSource
// ---------------------------------------------------------------------------

/// `net.minecraft.util.RandomSource` — the random-source interface.
///
/// Java interface methods that return `RandomSource` / `PositionalRandomFactory`
/// become `Self`/associated-type returns here; the `create*` static factories
/// are free functions at the bottom of this module.
pub trait RandomSource: Sized {
    /// `RandomSource.GAUSSIAN_SPREAD_FACTOR` (deprecated, retained).
    const GAUSSIAN_SPREAD_FACTOR: f64 = 2.297;

    /// The concrete positional factory type returned by `fork_positional`.
    ///
    /// Deliberately *not* constrained to `Output = Self`: Java
    /// `SingleThreadedRandomSource.forkPositional()` and
    /// `ThreadSafeLegacyRandomSource.forkPositional()` return a
    /// `LegacyPositionalRandomFactory` whose `at()`/`fromHashOf()` yield a
    /// `LegacyRandomSource` — a different concrete type.
    type Positional: PositionalRandomFactory;

    /// `RandomSource.fork()` — a fresh source seeded from this one's next long.
    fn fork(&mut self) -> Self;

    /// `RandomSource.forkPositional()`.
    fn fork_positional(&mut self) -> Self::Positional;

    /// `RandomSource.setSeed(long)`.
    fn set_seed(&mut self, seed: i64);

    /// `RandomSource.nextInt()`.
    fn next_int(&mut self) -> i32;

    /// `RandomSource.nextInt(int bound)` — the bounded form, `[0, bound)`.
    fn next_int_bound(&mut self, bound: i32) -> i32;

    /// `RandomSource.nextLong()`.
    fn next_long(&mut self) -> i64;

    /// `RandomSource.nextBoolean()`.
    fn next_boolean(&mut self) -> bool;

    /// `RandomSource.nextFloat()` — `[0, 1)`.
    fn next_float(&mut self) -> f32;

    /// `RandomSource.nextDouble()` — `[0, 1)`.
    fn next_double(&mut self) -> f64;

    /// `RandomSource.nextGaussian()` — Marsaglia-Polar with the stored-value
    /// quirk (see `MarsagliaPolarGaussian`).
    fn next_gaussian(&mut self) -> f64;

    /// `RandomSource.nextIntBetweenInclusive(min, maxInclusive)`.
    fn next_int_between_inclusive(&mut self, min: i32, max_inclusive: i32) -> i32 {
        // Java int arithmetic wraps.
        self.next_int_bound(max_inclusive.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min)
    }

    /// `RandomSource.triangle(double mean, double spread)`.
    fn triangle_f64(&mut self, mean: f64, spread: f64) -> f64 {
        mean + spread * (self.next_double() - self.next_double())
    }

    /// `RandomSource.triangle(float mean, float spread)`.
    fn triangle_f32(&mut self, mean: f32, spread: f32) -> f32 {
        mean + spread * (self.next_float() - self.next_float())
    }

    /// `RandomSource.consumeCount(int rounds)` — default: `nextInt()` per round.
    /// (XoroshiroRandomSource overrides this; see there.)
    fn consume_count(&mut self, rounds: i32) {
        for _ in 0..rounds {
            self.next_int();
        }
    }

    /// `RandomSource.nextInt(int origin, int bound)` — `[origin, bound)`.
    fn next_int_origin_bound(&mut self, origin: i32, bound: i32) -> i32 {
        if origin >= bound {
            panic!("bound - origin is non positive");
        }
        // Java `bound - origin` / `origin + ...` are int-wrapping; if the range
        // wraps to a non-positive value, `nextInt(range)` throws "Bound must be
        // positive" exactly as Java's `nextInt(bound)` would.
        let range = bound.wrapping_sub(origin);
        origin.wrapping_add(self.next_int_bound(range))
    }
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.BitRandomSource
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.BitRandomSource` — the bit-oriented
/// LCG layer. `next(bits)` feeds every other method's default.
pub trait BitRandomSource: RandomSource {
    /// `BitRandomSource.FLOAT_MULTIPLIER = 5.9604645E-8F` (= 2^-24).
    const FLOAT_MULTIPLIER: f32 = 5.9604645E-8;

    /// `BitRandomSource.DOUBLE_MULTIPLIER = 1.110223E-16F` — the float literal
    /// `1.110223E-16F` widened to double, exactly as the Java constant.
    const DOUBLE_MULTIPLIER: f64 = 1.110223e-16f32 as f64;

    /// `BitRandomSource.next(int bits)` — the raw next-`bits`-bits primitive.
    fn next(&mut self, bits: i32) -> i32;

    fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            panic!("Bound must be positive");
        }

        if (bound & (bound - 1)) == 0 {
            return (((bound as i64) * self.next(31) as i64) >> 31) as i32;
        }

        loop {
            let sample = self.next(31);
            let modulo = sample % bound;
            // Java `sample - modulo + (bound - 1) < 0`; int arithmetic wraps.
            if sample.wrapping_sub(modulo).wrapping_add(bound - 1) >= 0 {
                break modulo;
            }
        }
    }

    fn next_long(&mut self) -> i64 {
        let upper = self.next(32);
        let lower = self.next(32);
        ((upper as i64) << 32).wrapping_add(lower as i64)
    }

    fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * 5.9604645E-8_f32
    }

    fn next_double(&mut self) -> f64 {
        let upper = self.next(26);
        let lower = self.next(27);
        let combined = ((upper as i64) << 27).wrapping_add(lower as i64);
        // Java `combined * 1.110223E-16F`: long*float -> f32 multiply, widened.
        ((combined as f32) * (Self::DOUBLE_MULTIPLIER as f32)) as f64
    }
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.PositionalRandomFactory
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.PositionalRandomFactory` — yields a
/// source per position / seed / name.
///
/// The Java default overloads taking `BlockPos` / `Identifier` are omitted here
/// (those types live in rivet-core / rivet-registry, not yet ported); the
/// `at(x, y, z)` and `fromHashOf(String)` forms they delegate to are provided.
// `from_hash_of`/`from_seed` are faithful mirrors of the Java instance methods
// `fromHashOf`/`fromSeed`, so the `from_*`-taking-`&self` convention lint is a
// false positive here (renaming would break API fidelity).
#[allow(clippy::wrong_self_convention)]
pub trait PositionalRandomFactory {
    /// The concrete `RandomSource` type this factory produces.
    type Output: RandomSource;

    /// `PositionalRandomFactory.at(int x, int y, int z)`.
    fn at(&self, x: i32, y: i32, z: i32) -> Self::Output;

    /// `PositionalRandomFactory.fromHashOf(String name)`.
    fn from_hash_of(&self, name: &str) -> Self::Output;

    /// `PositionalRandomFactory.fromSeed(long seed)`.
    fn from_seed(&self, seed: i64) -> Self::Output;

    /// `PositionalRandomFactory.parityConfigString(StringBuilder sb)` — the
    /// `@VisibleForTesting` config string.
    fn parity_config_string(&self, sb: &mut String);
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.RandomSupport
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.RandomSupport` — seed upgrades, md5
/// name hashing, and the unique-seed generator.
pub mod random_support {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// `RandomSupport.GOLDEN_RATIO_64`.
    pub const GOLDEN_RATIO_64: i64 = -7046029254386353131;
    /// `RandomSupport.SILVER_RATIO_64`.
    pub const SILVER_RATIO_64: i64 = 7640891576956012809;

    /// `RandomSupport.SEED_UNIQUIFIER` — the atomic multiplier state backing
    /// `generateUniqueSeed()`.
    static SEED_UNIQUIFIER: AtomicI64 = AtomicI64::new(8682522807148012);

    /// `RandomSupport.mixStafford13(long z)` — the 64-bit Stafford Mix13
    /// finalizer used for seed mixing.
    pub fn mix_stafford13(mut z: i64) -> i64 {
        z = (z ^ ((z as u64 >> 30) as i64)).wrapping_mul(-4658895280553007687);
        z = (z ^ ((z as u64 >> 27) as i64)).wrapping_mul(-7723592293110705685);
        z ^ (z as u64 >> 31) as i64
    }

    /// `RandomSupport.upgradeSeedTo128bitUnmixed(long)`.
    pub fn upgrade_seed_to_128bit_unmixed(legacy_seed: i64) -> Seed128bit {
        let low_bits = legacy_seed ^ SILVER_RATIO_64;
        let high_bits = low_bits.wrapping_add(GOLDEN_RATIO_64);
        Seed128bit::new(low_bits, high_bits)
    }

    /// `RandomSupport.upgradeSeedTo128bit(long)`.
    pub fn upgrade_seed_to_128bit(legacy_seed: i64) -> Seed128bit {
        upgrade_seed_to_128bit_unmixed(legacy_seed).mixed()
    }

    /// `RandomSupport.seedFromHashOf(String)` — the md5 of the UTF-8 bytes,
    /// split big-endian into `(seedLo, seedHi)` exactly like Guava
    /// `Hashing.md5()` + `Longs.fromBytes`.
    pub fn seed_from_hash_of(input: &str) -> Seed128bit {
        let digest = md5::compute(input.as_bytes());
        let bytes = digest.0;
        let mut lo: i64 = 0;
        let mut hi: i64 = 0;
        for &b in &bytes[0..8] {
            lo = (lo << 8) | (b as i64 & 0xFF);
        }
        for &b in &bytes[8..16] {
            hi = (hi << 8) | (b as i64 & 0xFF);
        }
        Seed128bit::new(lo, hi)
    }

    /// `RandomSupport.generateUniqueSeed()` — the atomic-uniquifier seed XOR a
    /// time value. Java uses `System.nanoTime()`; we use wall-clock nanos, which
    /// preserves the "fresh unique seed" contract (the exact value is not
    /// observable/parity-relevant).
    pub fn generate_unique_seed() -> i64 {
        // fetch_update returns the PRE-update value, whereas Java's
        // AtomicLong.updateAndGet returns the POST-update value — so multiply
        // the returned value to match Java exactly.
        let previous = SEED_UNIQUIFIER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.wrapping_mul(1181783497276652981))
            })
            .unwrap_or_else(|_| unreachable!("fetch_update never rejects the closure result"));
        let updated = previous.wrapping_mul(1181783497276652981);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;
        updated ^ now
    }

    /// `RandomSupport.Seed128bit` — the immutable `(seedLo, seedHi)` record.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct Seed128bit {
        /// `seedLo()`.
        pub seed_lo: i64,
        /// `seedHi()`.
        pub seed_hi: i64,
    }

    impl Seed128bit {
        /// The record constructor.
        pub fn new(seed_lo: i64, seed_hi: i64) -> Self {
            Seed128bit { seed_lo, seed_hi }
        }

        /// `Seed128bit.xor(long lo, long hi)`.
        pub fn xor(&self, lo: i64, hi: i64) -> Self {
            Seed128bit::new(self.seed_lo ^ lo, self.seed_hi ^ hi)
        }

        /// `Seed128bit.xor(Seed128bit other)`.
        pub fn xor_seed(&self, other: Seed128bit) -> Self {
            self.xor(other.seed_lo, other.seed_hi)
        }

        /// `Seed128bit.mixed()` — both halves through `mixStafford13`.
        pub fn mixed(&self) -> Self {
            Seed128bit::new(mix_stafford13(self.seed_lo), mix_stafford13(self.seed_hi))
        }
    }
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.Xoroshiro128PlusPlus
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.Xoroshiro128PlusPlus` — the underlying
/// Xoroshiro128++ generator.
///
/// The `CODEC` (DFU `Codec.LONG_STREAM` xmap) is omitted: rivet-serialization
/// is not yet ported.
#[derive(Clone, Debug)]
pub struct Xoroshiro128PlusPlus {
    seed_lo: i64,
    seed_hi: i64,
}

impl Xoroshiro128PlusPlus {
    /// `Xoroshiro128PlusPlus(Seed128bit)`.
    pub fn new(seed: random_support::Seed128bit) -> Self {
        Self::new_lo_hi(seed.seed_lo, seed.seed_hi)
    }

    /// `Xoroshiro128PlusPlus(long seedLo, long seedHi)` — zero seeds are
    /// replaced by the golden/silver ratios so the state is never all-zero.
    pub fn new_lo_hi(seed_lo: i64, seed_hi: i64) -> Self {
        let (seed_lo, seed_hi) = if (seed_lo | seed_hi) == 0 {
            (
                random_support::GOLDEN_RATIO_64,
                random_support::SILVER_RATIO_64,
            )
        } else {
            (seed_lo, seed_hi)
        };
        Xoroshiro128PlusPlus { seed_lo, seed_hi }
    }

    /// `Xoroshiro128PlusPlus.nextLong()` — the Xoroshiro128++ step.
    pub fn next_long(&mut self) -> i64 {
        let s0 = self.seed_lo;
        let s1 = self.seed_hi;
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        let s1 = s1 ^ s0;
        self.seed_lo = s0.rotate_left(49) ^ s1 ^ (s1 << 21);
        self.seed_hi = s1.rotate_left(28);
        result
    }
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.MarsagliaPolarGaussian
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.MarsagliaPolarGaussian` — Gaussian
/// sampling with the stored-value quirk.
///
/// Java holds a `final RandomSource randomSource` referencing the owning source
/// (a self-reference). In Rust that maps to a method call where the cache is
/// swapped out of the source for the duration (see each source's
/// `next_gaussian`), so the cache here only stores the two quirk fields.
#[derive(Clone, Debug)]
pub struct MarsagliaPolarGaussian {
    /// `nextNextGaussian` — the stored second half of the polar pair.
    next_next_gaussian: f64,
    /// `haveNextNextGaussian`.
    have_next_next_gaussian: bool,
}

impl MarsagliaPolarGaussian {
    /// The no-arg constructor; `reset()` state.
    pub fn new() -> Self {
        MarsagliaPolarGaussian {
            next_next_gaussian: 0.0,
            have_next_next_gaussian: false,
        }
    }

    /// `MarsagliaPolarGaussian.reset()` — clears the stored value.
    pub fn reset(&mut self) {
        self.have_next_next_gaussian = false;
    }

    /// `MarsagliaPolarGaussian.nextGaussian()` against the given source.
    ///
    /// `radiusSquared == 0.0` is an exact f64 comparison, as in Java.
    pub fn next_gaussian<R: RandomSource>(&mut self, random_source: &mut R) -> f64 {
        if self.have_next_next_gaussian {
            self.have_next_next_gaussian = false;
            return self.next_next_gaussian;
        }

        let (x, y, radius_squared) = loop {
            let x = 2.0 * random_source.next_double() - 1.0;
            let y = 2.0 * random_source.next_double() - 1.0;
            let radius_squared = x * x + y * y;
            if radius_squared < 1.0 && radius_squared != 0.0 {
                break (x, y, radius_squared);
            }
        };

        let multiplier = (-2.0 * radius_squared.ln() / radius_squared).sqrt();
        self.next_next_gaussian = y * multiplier;
        self.have_next_next_gaussian = true;
        x * multiplier
    }
}

impl Default for MarsagliaPolarGaussian {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.LegacyRandomSource
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.LegacyRandomSource` — java.util.Random's
/// 48-bit LCG, bit-exact.
///
/// Threading: the Java `AtomicLong` + `ThreadingDetector` is dropped (tick-thread
/// confined, D5); `seed` is a plain field.
pub struct LegacyRandomSource {
    seed: i64,
    gaussian_source: MarsagliaPolarGaussian,
}

/// The LCG modulus, 2^48.
const LEGACY_MODULUS_BITS: i32 = 48;
/// `LegacyRandomSource.MODULUS_MASK` = 2^48 - 1.
const LEGACY_MODULUS_MASK: i64 = 281474976710655;
/// `LegacyRandomSource.MULTIPLIER` (java.util.Random's multiplier).
const LEGACY_MULTIPLIER: i64 = 25214903917;
/// `LegacyRandomSource.INCREMENT`.
const LEGACY_INCREMENT: i64 = 11;

impl LegacyRandomSource {
    /// `LegacyRandomSource(long seed)`.
    pub fn new(seed: i64) -> Self {
        let mut source = LegacyRandomSource {
            seed: 0,
            gaussian_source: MarsagliaPolarGaussian::new(),
        };
        source.set_seed(seed);
        source
    }

    fn next_gaussian_inner(&mut self) -> f64 {
        // Java's gaussianSource references `this`; swap the cache out so `self`
        // can be borrowed whole as the RNG source without aliasing the field.
        let mut gaussian = std::mem::take(&mut self.gaussian_source);
        let value = gaussian.next_gaussian(self);
        self.gaussian_source = gaussian;
        value
    }
}

impl RandomSource for LegacyRandomSource {
    type Positional = LegacyPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        LegacyRandomSource::new(<Self as BitRandomSource>::next_long(self))
    }

    fn fork_positional(&mut self) -> Self::Positional {
        LegacyPositionalRandomFactory::new(<Self as BitRandomSource>::next_long(self))
    }

    fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ LEGACY_MULTIPLIER) & LEGACY_MODULUS_MASK;
        self.gaussian_source.reset();
    }

    fn next_int(&mut self) -> i32 {
        <Self as BitRandomSource>::next_int(self)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        <Self as BitRandomSource>::next_int_bound(self, bound)
    }

    fn next_long(&mut self) -> i64 {
        <Self as BitRandomSource>::next_long(self)
    }

    fn next_boolean(&mut self) -> bool {
        <Self as BitRandomSource>::next_boolean(self)
    }

    fn next_float(&mut self) -> f32 {
        <Self as BitRandomSource>::next_float(self)
    }

    fn next_double(&mut self) -> f64 {
        <Self as BitRandomSource>::next_double(self)
    }

    fn next_gaussian(&mut self) -> f64 {
        self.next_gaussian_inner()
    }
}

impl BitRandomSource for LegacyRandomSource {
    /// `LegacyRandomSource.next(int bits)` — the LCG step, returning the top
    /// `bits` bits of the new seed.
    fn next(&mut self, bits: i32) -> i32 {
        let new_seed = self
            .seed
            .wrapping_mul(LEGACY_MULTIPLIER)
            .wrapping_add(LEGACY_INCREMENT)
            & LEGACY_MODULUS_MASK;
        self.seed = new_seed;
        (new_seed >> (LEGACY_MODULUS_BITS - bits)) as i32
    }
}

/// `LegacyRandomSource.LegacyPositionalRandomFactory`.
#[derive(Clone, Copy, Debug)]
pub struct LegacyPositionalRandomFactory {
    seed: i64,
}

impl LegacyPositionalRandomFactory {
    /// `LegacyPositionalRandomFactory(long seed)`.
    pub fn new(seed: i64) -> Self {
        LegacyPositionalRandomFactory { seed }
    }
}

impl PositionalRandomFactory for LegacyPositionalRandomFactory {
    type Output = LegacyRandomSource;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Output {
        let positional_seed = get_seed(x, y, z);
        LegacyRandomSource::new(positional_seed ^ self.seed)
    }

    fn from_hash_of(&self, name: &str) -> Self::Output {
        let positional_seed = java_string_hash(name);
        LegacyRandomSource::new((positional_seed as i64) ^ self.seed)
    }

    fn from_seed(&self, seed: i64) -> Self::Output {
        LegacyRandomSource::new(seed)
    }

    fn parity_config_string(&self, sb: &mut String) {
        sb.push_str("LegacyPositionalRandomFactory{");
        sb.push_str(&self.seed.to_string());
        sb.push('}');
    }
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.XoroshiroRandomSource
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.XoroshiroRandomSource`.
///
/// Implements `RandomSource` directly (not `BitRandomSource`), exactly as the
/// Java class.
pub struct XoroshiroRandomSource {
    random_number_generator: Xoroshiro128PlusPlus,
    gaussian_source: MarsagliaPolarGaussian,
}

impl XoroshiroRandomSource {
    /// `XoroshiroRandomSource(long seed)` — seeds via
    /// `RandomSupport.upgradeSeedTo128bit`.
    pub fn new(seed: i64) -> Self {
        let generator = Xoroshiro128PlusPlus::new(random_support::upgrade_seed_to_128bit(seed));
        XoroshiroRandomSource {
            random_number_generator: generator,
            gaussian_source: MarsagliaPolarGaussian::new(),
        }
    }

    /// `XoroshiroRandomSource(Seed128bit)`.
    pub fn from_seed128(seed: random_support::Seed128bit) -> Self {
        Self::from_generator(Xoroshiro128PlusPlus::new(seed))
    }

    /// `XoroshiroRandomSource(long seedLo, long seedHi)`.
    pub fn new_lo_hi(seed_lo: i64, seed_hi: i64) -> Self {
        Self::from_generator(Xoroshiro128PlusPlus::new_lo_hi(seed_lo, seed_hi))
    }

    /// The private `XoroshiroRandomSource(Xoroshiro128PlusPlus)` constructor.
    pub(crate) fn from_generator(random_number_generator: Xoroshiro128PlusPlus) -> Self {
        XoroshiroRandomSource {
            random_number_generator,
            gaussian_source: MarsagliaPolarGaussian::new(),
        }
    }

    /// `XoroshiroRandomSource.nextBits(int bits)` — `nextLong() >>> (64 - bits)`.
    fn next_bits(&mut self, bits: i32) -> i64 {
        (self.random_number_generator.next_long() as u64 >> (64 - bits)) as i64
    }

    fn next_gaussian_inner(&mut self) -> f64 {
        let mut gaussian = std::mem::take(&mut self.gaussian_source);
        let value = gaussian.next_gaussian(self);
        self.gaussian_source = gaussian;
        value
    }
}

impl RandomSource for XoroshiroRandomSource {
    type Positional = XoroshiroPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        XoroshiroRandomSource::new_lo_hi(
            self.random_number_generator.next_long(),
            self.random_number_generator.next_long(),
        )
    }

    fn fork_positional(&mut self) -> Self::Positional {
        XoroshiroPositionalRandomFactory::new(
            self.random_number_generator.next_long(),
            self.random_number_generator.next_long(),
        )
    }

    fn set_seed(&mut self, seed: i64) {
        self.random_number_generator =
            Xoroshiro128PlusPlus::new(random_support::upgrade_seed_to_128bit(seed));
        self.gaussian_source.reset();
    }

    fn next_int(&mut self) -> i32 {
        self.random_number_generator.next_long() as i32
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            panic!("Bound must be positive");
        }

        let mut random_bits = self.next_int() as u32 as i64; // Integer.toUnsignedLong
        let mut multiplied_random_bits = random_bits.wrapping_mul(bound as i64);
        let mut fractional_part = multiplied_random_bits & 0xFFFF_FFFF;

        if fractional_part < bound as i64 {
            // Integer.remainderUnsigned(~bound + 1, bound); ~bound + 1 == -bound.
            let unbiased_buckets_start_index =
                ((bound.wrapping_neg() as u32) % (bound as u32)) as i32;
            while fractional_part < unbiased_buckets_start_index as i64 {
                random_bits = self.next_int() as u32 as i64;
                multiplied_random_bits = random_bits.wrapping_mul(bound as i64);
                fractional_part = multiplied_random_bits & 0xFFFF_FFFF;
            }
        }

        (multiplied_random_bits >> 32) as i32
    }

    fn next_long(&mut self) -> i64 {
        self.random_number_generator.next_long()
    }

    fn next_boolean(&mut self) -> bool {
        (self.random_number_generator.next_long() & 1) != 0
    }

    fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 * 5.9604645E-8_f32
    }

    fn next_double(&mut self) -> f64 {
        // Java `nextBits(53) * 1.110223E-16F`: long*float -> f32 multiply, widened.
        ((self.next_bits(53) as f32) * 1.110223e-16f32) as f64
    }

    fn next_gaussian(&mut self) -> f64 {
        self.next_gaussian_inner()
    }

    /// `XoroshiroRandomSource.consumeCount(int rounds)` — consumes generator
    /// longs directly (NOT `nextInt()` like the interface default).
    fn consume_count(&mut self, rounds: i32) {
        for _ in 0..rounds {
            self.random_number_generator.next_long();
        }
    }
}

/// `XoroshiroRandomSource.XoroshiroPositionalRandomFactory`.
#[derive(Clone, Copy, Debug)]
pub struct XoroshiroPositionalRandomFactory {
    seed_lo: i64,
    seed_hi: i64,
}

impl XoroshiroPositionalRandomFactory {
    /// `XoroshiroPositionalRandomFactory(long seedLo, long seedHi)`.
    pub fn new(seed_lo: i64, seed_hi: i64) -> Self {
        XoroshiroPositionalRandomFactory { seed_lo, seed_hi }
    }
}

impl PositionalRandomFactory for XoroshiroPositionalRandomFactory {
    type Output = XoroshiroRandomSource;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Output {
        let positional_seed = get_seed(x, y, z);
        XoroshiroRandomSource::new_lo_hi(positional_seed ^ self.seed_lo, self.seed_hi)
    }

    fn from_hash_of(&self, name: &str) -> Self::Output {
        let seed = random_support::seed_from_hash_of(name);
        XoroshiroRandomSource::from_seed128(seed.xor(self.seed_lo, self.seed_hi))
    }

    fn from_seed(&self, seed: i64) -> Self::Output {
        XoroshiroRandomSource::new_lo_hi(seed ^ self.seed_lo, seed ^ self.seed_hi)
    }

    fn parity_config_string(&self, sb: &mut String) {
        sb.push_str("seedLo: ");
        sb.push_str(&self.seed_lo.to_string());
        sb.push_str(", seedHi: ");
        sb.push_str(&self.seed_hi.to_string());
    }
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.SingleThreadedRandomSource
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.SingleThreadedRandomSource` — like
/// LegacyRandomSource but with a plain (non-atomic) seed and a lazily-created
/// Gaussian source.
pub struct SingleThreadedRandomSource {
    seed: i64,
    gaussian_source: Option<MarsagliaPolarGaussian>,
}

impl SingleThreadedRandomSource {
    /// `SingleThreadedRandomSource(long seed)`.
    pub fn new(seed: i64) -> Self {
        let mut source = SingleThreadedRandomSource {
            seed: 0,
            gaussian_source: None,
        };
        source.set_seed(seed);
        source
    }

    fn next_gaussian_inner(&mut self) -> f64 {
        let mut gaussian = self.gaussian_source.take().unwrap_or_default();
        let value = gaussian.next_gaussian(self);
        self.gaussian_source = Some(gaussian);
        value
    }
}

impl RandomSource for SingleThreadedRandomSource {
    type Positional = LegacyPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        SingleThreadedRandomSource::new(<Self as BitRandomSource>::next_long(self))
    }

    fn fork_positional(&mut self) -> Self::Positional {
        LegacyPositionalRandomFactory::new(<Self as BitRandomSource>::next_long(self))
    }

    fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ LEGACY_MULTIPLIER) & LEGACY_MODULUS_MASK;
        if let Some(gaussian) = &mut self.gaussian_source {
            gaussian.reset();
        }
    }

    fn next_int(&mut self) -> i32 {
        <Self as BitRandomSource>::next_int(self)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        <Self as BitRandomSource>::next_int_bound(self, bound)
    }

    fn next_long(&mut self) -> i64 {
        <Self as BitRandomSource>::next_long(self)
    }

    fn next_boolean(&mut self) -> bool {
        <Self as BitRandomSource>::next_boolean(self)
    }

    fn next_float(&mut self) -> f32 {
        <Self as BitRandomSource>::next_float(self)
    }

    fn next_double(&mut self) -> f64 {
        <Self as BitRandomSource>::next_double(self)
    }

    fn next_gaussian(&mut self) -> f64 {
        self.next_gaussian_inner()
    }
}

impl BitRandomSource for SingleThreadedRandomSource {
    fn next(&mut self, bits: i32) -> i32 {
        let new_seed = self
            .seed
            .wrapping_mul(LEGACY_MULTIPLIER)
            .wrapping_add(LEGACY_INCREMENT)
            & LEGACY_MODULUS_MASK;
        self.seed = new_seed;
        (new_seed >> (LEGACY_MODULUS_BITS - bits)) as i32
    }
}

// ---------------------------------------------------------------------------
// net.minecraft.world.level.levelgen.ThreadSafeLegacyRandomSource
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.ThreadSafeLegacyRandomSource` — the
/// deprecated atomic-LCG variant used by `RandomSource.createThreadSafe()`.
///
/// Unlike `LegacyRandomSource`, the seed stays an `AtomicI64` and `setSeed`
/// does NOT reset the Gaussian source (faithful to the Java). `next` uses
/// `>>>` (logical shift) — identical here to `>>` because the seed is
/// non-negative, but preserved as written.
pub struct ThreadSafeLegacyRandomSource {
    seed: AtomicI64,
    gaussian_source: MarsagliaPolarGaussian,
}

impl ThreadSafeLegacyRandomSource {
    /// `ThreadSafeLegacyRandomSource(long seed)`.
    pub fn new(seed: i64) -> Self {
        let mut source = ThreadSafeLegacyRandomSource {
            seed: AtomicI64::new(0),
            gaussian_source: MarsagliaPolarGaussian::new(),
        };
        source.set_seed(seed);
        source
    }

    fn next_gaussian_inner(&mut self) -> f64 {
        let mut gaussian = std::mem::take(&mut self.gaussian_source);
        let value = gaussian.next_gaussian(self);
        self.gaussian_source = gaussian;
        value
    }
}

impl RandomSource for ThreadSafeLegacyRandomSource {
    type Positional = LegacyPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        ThreadSafeLegacyRandomSource::new(<Self as BitRandomSource>::next_long(self))
    }

    fn fork_positional(&mut self) -> Self::Positional {
        LegacyPositionalRandomFactory::new(<Self as BitRandomSource>::next_long(self))
    }

    fn set_seed(&mut self, seed: i64) {
        // Java `this.seed.set(...)` — no Gaussian reset.
        self.seed.store(
            (seed ^ LEGACY_MULTIPLIER) & LEGACY_MODULUS_MASK,
            Ordering::Relaxed,
        );
    }

    fn next_int(&mut self) -> i32 {
        <Self as BitRandomSource>::next_int(self)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        <Self as BitRandomSource>::next_int_bound(self, bound)
    }

    fn next_long(&mut self) -> i64 {
        <Self as BitRandomSource>::next_long(self)
    }

    fn next_boolean(&mut self) -> bool {
        <Self as BitRandomSource>::next_boolean(self)
    }

    fn next_float(&mut self) -> f32 {
        <Self as BitRandomSource>::next_float(self)
    }

    fn next_double(&mut self) -> f64 {
        <Self as BitRandomSource>::next_double(self)
    }

    fn next_gaussian(&mut self) -> f64 {
        self.next_gaussian_inner()
    }
}

impl BitRandomSource for ThreadSafeLegacyRandomSource {
    fn next(&mut self, bits: i32) -> i32 {
        loop {
            let old_seed = self.seed.load(Ordering::Relaxed);
            let next_seed = old_seed
                .wrapping_mul(LEGACY_MULTIPLIER)
                .wrapping_add(LEGACY_INCREMENT)
                & LEGACY_MODULUS_MASK;
            if self
                .seed
                .compare_exchange(old_seed, next_seed, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return ((next_seed as u64) >> (LEGACY_MODULUS_BITS - bits)) as i32;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RandomSource static factories
// ---------------------------------------------------------------------------

/// `RandomSource.create()` — a `LegacyRandomSource` with a unique seed.
pub fn random_source_create() -> LegacyRandomSource {
    LegacyRandomSource::new(random_support::generate_unique_seed())
}

/// `RandomSource.create(long seed)`.
pub fn random_source_create_with_seed(seed: i64) -> LegacyRandomSource {
    LegacyRandomSource::new(seed)
}

/// `RandomSource.createThreadSafe()`.
pub fn random_source_create_thread_safe() -> ThreadSafeLegacyRandomSource {
    ThreadSafeLegacyRandomSource::new(random_support::generate_unique_seed())
}

/// `RandomSource.createThreadLocalInstance(long seed)`.
pub fn random_source_create_thread_local_instance_with_seed(
    seed: i64,
) -> SingleThreadedRandomSource {
    SingleThreadedRandomSource::new(seed)
}

/// `RandomSource.createThreadLocalInstance()` — Java seeds from netty's
/// `ThreadLocalRandom`; we use `generateUniqueSeed()` instead (same "fresh
/// unique seed" contract; the exact source is not observable).
pub fn random_source_create_thread_local_instance() -> SingleThreadedRandomSource {
    SingleThreadedRandomSource::new(random_support::generate_unique_seed())
}

#[cfg(test)]
mod tests {
    use super::random_support::{mix_stafford13, seed_from_hash_of, upgrade_seed_to_128bit};
    use super::{
        LegacyPositionalRandomFactory, LegacyRandomSource, PositionalRandomFactory, RandomSource,
        SingleThreadedRandomSource, ThreadSafeLegacyRandomSource, Xoroshiro128PlusPlus,
        XoroshiroPositionalRandomFactory, XoroshiroRandomSource, random_source_create,
        random_source_create_thread_local_instance, random_source_create_thread_safe,
    };

    // --- RandomSupport / Xoroshiro128PlusPlus goldens (OpenJDK 25) ---

    #[test]
    fn mix_stafford13_golden() {
        for (input, expected) in [
            (0, 0),
            (1, 6238072747940578789),
            (-1, -5417735806833148549),
            (12345, -906084347102765743),
            (-7046029254386353131, -2152535657050944081),
            (7640891576956012809, 3847398142028685078),
            (8682522807148012, -164420965669769191),
            (281474976710655, -9141449195665599062),
            (i64::MAX, 6514504133438201533),
            (i64::MIN, 2720858781877447050),
            (42, -6387817139659442654),
            (999999999999, -2277723687273847764),
        ] {
            assert_eq!(mix_stafford13(input), expected, "mix({input})");
        }
    }

    #[test]
    fn upgrade_seed_to_128bit_golden() {
        for (input, lo, hi) in [
            (0, 3847398142028685078, 7192185014346937746),
            (1, 5272463233947570727, 1927618558350093866),
            (-1, -110783831392733308, 2932223646667407290),
            (12345, 733019005196230046, -3494074583369400597),
            (281474976710655, 8218345850782293387, 8589122113358971107),
            (i64::MIN, -6382634648412944878, 5448932524140013571),
        ] {
            let s = upgrade_seed_to_128bit(input);
            assert_eq!((s.seed_lo, s.seed_hi), (lo, hi), "upgrade({input})");
        }
    }

    #[test]
    fn seed_from_hash_of_golden() {
        for (input, lo, hi) in [
            ("", -3162216497309240828, -1621285313438006658),
            ("minecraft", -7427765047119558610, 2710366963000269281),
            (
                "minecraft:overworld",
                4754257565590824948,
                -1113069127075346987,
            ),
            ("hello", 6719722671305337462, -5084109257958767214),
            ("random.seed", 8890460171222637671, 7039033920110803771),
            ("The End", 5063799902454280945, 6589391831577508400),
            ("a\u{0}b", 8085385611671561073, 4569876295026225307),
        ] {
            let s = seed_from_hash_of(input);
            assert_eq!((s.seed_lo, s.seed_hi), (lo, hi), "hash({input:?})");
        }
    }

    #[test]
    fn xoroshiro128plusplus_raw_golden() {
        let mut x = Xoroshiro128PlusPlus::new_lo_hi(1, 2);
        for expected in [
            393217,
            669327710093319,
            1732421326133921491,
            -7051953992050424633,
            -8891291296936358940,
            3586421180005889563,
            1691397964866707553,
            -7711117276956439919,
        ] {
            assert_eq!(x.next_long(), expected);
        }
    }

    #[test]
    fn xoroshiro128plusplus_zero_seed_uses_golden_ratio() {
        let mut x = Xoroshiro128PlusPlus::new_lo_hi(0, 0);
        assert_eq!(x.next_long(), 6807859099481836695);
        assert_eq!(x.next_long(), 5275285228792843439);
    }

    // --- LegacyRandomSource sequences ---

    fn legacy_seq(
        seed: i64,
        ints: &[i32],
        longs: &[i64],
        floats: &[u32],
        doubles: &[u64],
        gaussians: &[u64],
    ) {
        let mut r = LegacyRandomSource::new(seed);
        for &e in ints {
            assert_eq!(r.next_int(), e);
        }
        for &e in longs {
            assert_eq!(r.next_long(), e);
        }
        for &e in floats {
            assert_eq!(r.next_float().to_bits(), e);
        }
        for &e in doubles {
            assert_eq!(r.next_double().to_bits(), e);
        }
        for &e in gaussians {
            assert_eq!(r.next_gaussian().to_bits(), e);
        }
    }

    #[test]
    fn legacy_seed_12345_sequence() {
        legacy_seq(
            12345,
            &[1553932502, -2090749135, -287790814, -355989640, -716867186],
            &[
                694943615613659775,
                2299721358978533787,
                651833349628731322,
                -220423784200063172,
                -1589248208719184439,
            ],
            &[0x3decbb18, 0x3e22109c, 0x3f6e03f4, 0x3f44d5d9, 0x3e8679f8],
            &[
                0x3fee3e0ac0000000,
                0x3fb0ca2e60000000,
                0x3fe9ed6a20000000,
                0x3fde962a60000000,
                0x3fd5491880000000,
            ],
            &[
                0x3fcf9897addd8c3f,
                0x3fe113a2271488b5,
                0xbfd031cf5c76af5d,
                0xbfe63f50fd5cdad0,
                0x3fc09a96d22540b3,
                0xbfedc3484f722d83,
            ],
        );
    }

    #[test]
    fn legacy_seed_0_sequence() {
        legacy_seq(
            0,
            &[-1155484576, -723955400, 1033096058],
            &[
                -7261648964369397258,
                5700976833288827063,
                2158390814503909950,
            ],
            &[0x3f4812a7, 0x3eaa9b9a, 0x3e816be0],
            &[0x3fd8a6f080000000, 0x3fef83d260000000, 0x3fec224360000000],
            &[
                0x3fc61aa4b06e4fcc,
                0xbfb68c0bb6b9c89b,
                0xbfda5b698efadcbc,
                0x3fa4abe814603fb7,
            ],
        );
    }

    #[test]
    fn legacy_seed_minus_1_and_max_mask_are_identical() {
        // setSeed masks with & MODULUS_MASK, so -1 and 2^48-1 seed identically.
        legacy_seq(
            -1,
            &[1155099827, 1887904451, 52699159],
            &[
                -8337289232527761815,
                -7364023878800395366,
                7365572370171403210,
            ],
            &[0x3d679380, 0x3f4e5875, 0x3e9e99d6],
            &[0x3fad138000000000, 0x3fb51643e0000000, 0x3fe72242c0000000],
            &[
                0x3ff3ddc5f0d7fbf6,
                0x40009ed6c0eeea74,
                0x3fe49664ed108e47,
                0x3fc74bc9e81cadeb,
            ],
        );
        legacy_seq(
            281474976710655,
            &[1155099827, 1887904451, 52699159],
            &[
                -8337289232527761815,
                -7364023878800395366,
                7365572370171403210,
            ],
            &[0x3d679380, 0x3f4e5875, 0x3e9e99d6],
            &[0x3fad138000000000, 0x3fb51643e0000000, 0x3fe72242c0000000],
            &[
                0x3ff3ddc5f0d7fbf6,
                0x40009ed6c0eeea74,
                0x3fe49664ed108e47,
                0x3fc74bc9e81cadeb,
            ],
        );
    }

    #[test]
    fn legacy_next_int_bound_golden() {
        let mut r = LegacyRandomSource::new(42);
        for (bound, expected) in [
            (1, [0, 0, 0, 0, 0]),
            (2, [1, 0, 1, 1, 0]),
            (3, [2, 2, 0, 0, 2]),
            (5, [2, 1, 0, 3, 4]),
            (100, [0, 63, 26, 13, 43]),
            (12345, [7881, 6505, 6688, 67, 7386]),
            (
                i32::MAX,
                [1276356730, 726510756, 450472430, 538291085, 1773748217],
            ),
        ] {
            for &e in &expected {
                assert_eq!(r.next_int_bound(bound), e, "bound {bound}");
            }
        }
    }

    // --- XoroshiroRandomSource sequences ---

    fn xoroshiro_seq(
        seed: i64,
        ints: &[i32],
        longs: &[i64],
        floats: &[u32],
        doubles: &[u64],
        gaussians: &[u64],
    ) {
        let mut r = XoroshiroRandomSource::new(seed);
        for &e in ints {
            assert_eq!(r.next_int(), e);
        }
        for &e in longs {
            assert_eq!(r.next_long(), e);
        }
        for &e in floats {
            assert_eq!(r.next_float().to_bits(), e);
        }
        for &e in doubles {
            assert_eq!(r.next_double().to_bits(), e);
        }
        for &e in gaussians {
            assert_eq!(r.next_gaussian().to_bits(), e);
        }
    }

    #[test]
    fn xoroshiro_seed_12345_sequence() {
        xoroshiro_seq(
            12345,
            &[57184507, -778892930, -527878333, 79047081, -1911977516],
            &[
                3531070785145858642,
                1108557117547011729,
                -3481306480006774438,
                -7392783547274240084,
                6109844536179375000,
            ],
            &[0x3ebe1eb6, 0x3e981e14, 0x3ef7350a, 0x3cd3afe0, 0x3f487a11],
            &[
                0x3fe4c3ff80000000,
                0x3fe9849860000000,
                0x3fda534da0000000,
                0x3fdb603560000000,
                0x3fec03cf80000000,
            ],
            &[
                0xbfe633db74ffa45f,
                0x3fef55e3f17ea087,
                0xbfc07c26a83d0703,
                0x3ffee3fd8728a7f7,
                0xbff6a15aaeb84a8f,
                0xbfe900c26d1146c0,
            ],
        );
    }

    #[test]
    fn xoroshiro_seed_0_sequence() {
        xoroshiro_seq(
            0,
            &[-160476802, 781697906, 653572596],
            &[
                2160572957309072155,
                1839370574944072389,
                -4488466507718817201,
            ],
            &[0x3f45b753, 0x3f7129fd, 0x3ef60c38],
            &[0x3fe384dde0000000, 0x3fc1e77080000000, 0x3fded1be20000000],
            &[
                0xbfd979c205c7e8ed,
                0xbfc9d74634253311,
                0x3fd93f4ebc1b6b92,
                0x3fb60c04e3b9d23f,
            ],
        );
    }

    #[test]
    fn xoroshiro_seed_minus_1_sequence() {
        xoroshiro_seq(
            -1,
            &[-1451817517, 2009620260, 916420186],
            &[
                -2068491455652362927,
                -5626054917968568837,
                350347487066691045,
            ],
            &[0x39a3a000, 0x3f507c5a, 0x3f133086],
            &[0x3fcc585f40000000, 0x3fe1320080000000, 0x3fe86267e0000000],
            &[
                0x3fe46b075208216e,
                0x3fe015024f28ce02,
                0xbff90ec3188bb798,
                0xbff3ebe3812d37bd,
            ],
        );
    }

    #[test]
    fn xoroshiro_lo_hi_67890_101010_sequence() {
        let mut r = XoroshiroRandomSource::new_lo_hi(67890, 101010);
        assert_eq!(
            [r.next_int(), r.next_int(), r.next_int()],
            [663292210, 2068097130, 1487657728]
        );
        assert_eq!(
            [r.next_long(), r.next_long(), r.next_long()],
            [
                1761220515690521315,
                9113115160302517150,
                -1969232521261176585
            ]
        );
        assert_eq!(
            [
                r.next_float().to_bits(),
                r.next_float().to_bits(),
                r.next_float().to_bits()
            ],
            [0x3f250aa5, 0x3f4b8a10, 0x3f4ab210]
        );
        assert_eq!(
            [
                r.next_double().to_bits(),
                r.next_double().to_bits(),
                r.next_double().to_bits()
            ],
            [0x3fec329ca0000000, 0x3fdc6833a0000000, 0x3feb1ee7a0000000]
        );
        assert_eq!(
            [
                r.next_gaussian().to_bits(),
                r.next_gaussian().to_bits(),
                r.next_gaussian().to_bits(),
                r.next_gaussian().to_bits(),
            ],
            [
                0x4004437226e22b49,
                0x3fb6d9172dfa5705,
                0x3fc9afeb650617ae,
                0x400134b5552dc309,
            ]
        );
    }

    #[test]
    fn xoroshiro_next_int_bound_golden() {
        let mut r = XoroshiroRandomSource::new(42);
        for (bound, expected) in [
            (1, [0, 0, 0, 0, 0]),
            (2, [0, 1, 0, 1, 1]),
            (3, [2, 2, 2, 0, 0]),
            (5, [4, 1, 1, 1, 2]),
            (100, [38, 88, 27, 99, 78]),
            (12345, [6149, 9797, 6337, 12314, 5570]),
            (
                i32::MAX,
                [1031699345, 389394961, 1905577094, 656942939, 922069506],
            ),
        ] {
            for &e in &expected {
                assert_eq!(r.next_int_bound(bound), e, "bound {bound}");
            }
        }
    }

    // --- nextGaussian stored-value quirk + setSeed reset ---

    #[test]
    fn xoroshiro_set_seed_resets_gaussian() {
        let mut r = XoroshiroRandomSource::new(7);
        let first = r.next_gaussian().to_bits();
        let second = r.next_gaussian().to_bits();
        assert_ne!(first, second, "stored value must differ from fresh draw");
        r.set_seed(7);
        assert_eq!(
            r.next_gaussian().to_bits(),
            first,
            "setSeed must reset the stored value"
        );
    }

    #[test]
    fn legacy_set_seed_resets_gaussian() {
        let mut r = LegacyRandomSource::new(7);
        let first = r.next_gaussian().to_bits();
        let second = r.next_gaussian().to_bits();
        assert_ne!(first, second);
        r.set_seed(7);
        assert_eq!(r.next_gaussian().to_bits(), first);
    }

    #[test]
    fn gaussian_stored_value_golden() {
        let mut x = XoroshiroRandomSource::new(7);
        assert_eq!(x.next_gaussian().to_bits(), 0x3fc09e87e3a03555);
        assert_eq!(x.next_gaussian().to_bits(), 0x3fa3e3fdf67627cf);
        let mut l = LegacyRandomSource::new(7);
        assert_eq!(l.next_gaussian().to_bits(), 0x3feb0bedb6d2af2f);
        assert_eq!(l.next_gaussian().to_bits(), 0x3fed3647f3f65f1b);
    }

    // --- consumeCount / interface defaults ---

    #[test]
    fn consume_count_golden() {
        let mut x = XoroshiroRandomSource::new(7);
        x.consume_count(5);
        assert_eq!(x.next_int(), 2132829273);

        // Legacy uses the interface default: 5 x nextInt().
        let mut l = LegacyRandomSource::new(7);
        l.consume_count(5);
        assert_eq!(l.next_int(), 2107132509);
    }

    #[test]
    fn next_int_origin_bound_golden() {
        let mut x = XoroshiroRandomSource::new(1234);
        let got: Vec<i32> = (0..5).map(|_| x.next_int_origin_bound(10, 20)).collect();
        assert_eq!(got, vec![10, 19, 19, 19, 14]);
        let mut l = LegacyRandomSource::new(1234);
        let got: Vec<i32> = (0..5).map(|_| l.next_int_origin_bound(10, 20)).collect();
        assert_eq!(got, vec![18, 13, 13, 10, 10]);
    }

    #[test]
    fn next_int_between_inclusive_golden() {
        let mut x = XoroshiroRandomSource::new(5678);
        let got: Vec<i32> = (0..8)
            .map(|_| x.next_int_between_inclusive(5, 10))
            .collect();
        assert_eq!(got, vec![10, 8, 7, 8, 10, 7, 7, 10]);
    }

    #[test]
    fn triangle_golden() {
        // Xoroshiro/Legacy seed 111, golden from the real Java Paper
        // (net.minecraft.world.level.levelgen) via Double.doubleToRawLongBits.
        let mut x = XoroshiroRandomSource::new(111);
        assert_eq!(x.triangle_f64(2.0, 3.0).to_bits(), 0x3ff6ef7cd5640000);
        assert_eq!(x.triangle_f64(2.0, 3.0).to_bits(), 0x4004433b38000000);
        assert_eq!(x.triangle_f64(2.0, 3.0).to_bits(), 0x3fe811e300000000);
        let mut l = LegacyRandomSource::new(111);
        assert_eq!(l.triangle_f64(2.0, 3.0).to_bits(), 0x40082679e8000000);
        assert_eq!(l.triangle_f64(2.0, 3.0).to_bits(), 0x4009b756e8000000);
        assert_eq!(l.triangle_f64(2.0, 3.0).to_bits(), 0x3ffa14cf28000000);
    }

    #[test]
    fn next_boolean_golden() {
        let mut x = XoroshiroRandomSource::new(55);
        let got: Vec<bool> = (0..12).map(|_| x.next_boolean()).collect();
        assert_eq!(
            got,
            vec![
                false, false, true, false, false, false, true, true, true, true, false, false
            ]
        );
        let mut l = LegacyRandomSource::new(55);
        let got: Vec<bool> = (0..12).map(|_| l.next_boolean()).collect();
        assert_eq!(
            got,
            vec![
                true, true, false, true, false, false, false, false, true, false, false, false
            ]
        );
    }

    // --- fork / forkPositional / positional factories ---

    #[test]
    fn fork_golden() {
        let mut lf = LegacyRandomSource::new(99);
        let mut child = lf.fork();
        assert_eq!(
            [child.next_int(), child.next_int(), child.next_int()],
            [1487093057, -892544769, -415585755]
        );

        let mut xf = XoroshiroRandomSource::new(99);
        let mut xchild = xf.fork();
        assert_eq!(
            [xchild.next_int(), xchild.next_int(), xchild.next_int()],
            [-1398595983, 1906876314, -1060549825]
        );
    }

    #[test]
    fn legacy_positional_factory_golden() {
        let f = LegacyPositionalRandomFactory::new(99);
        // Each at() returns a fresh source with a deterministic first value.
        let mut r = f.at(1, 2, 3);
        assert_eq!(r.next_int(), -841563061);
        let mut r = f.at(1, 2, 3);
        assert_eq!(r.next_int(), -841563061);
        let mut r = f.from_hash_of("minecraft:overworld");
        assert_eq!(r.next_int(), 1060497612);
        let mut r = f.from_seed(42);
        assert_eq!(r.next_int(), -1170105035);
        let mut sb = String::new();
        f.parity_config_string(&mut sb);
        assert_eq!(sb, "LegacyPositionalRandomFactory{99}");
    }

    #[test]
    fn xoroshiro_positional_factory_golden() {
        let f = XoroshiroPositionalRandomFactory::new(99, 1234);
        let mut r = f.at(1, 2, 3);
        assert_eq!(r.next_int(), 508115354);
        let mut r = f.at(1, 2, 3);
        assert_eq!(r.next_int(), 508115354);
        let mut r = f.from_hash_of("minecraft:overworld");
        assert_eq!(r.next_int(), -386036569);
        let mut r = f.from_seed(42);
        assert_eq!(r.next_int(), 176291913);
        let mut sb = String::new();
        f.parity_config_string(&mut sb);
        assert_eq!(sb, "seedLo: 99, seedHi: 1234");
    }

    // --- other sources ---

    #[test]
    fn single_threaded_matches_legacy() {
        // Same LCG -> identical sequence to LegacyRandomSource.
        let mut st = SingleThreadedRandomSource::new(12345);
        let mut l = LegacyRandomSource::new(12345);
        for _ in 0..10 {
            assert_eq!(st.next_int(), l.next_int());
        }
        for _ in 0..10 {
            assert_eq!(st.next_long(), l.next_long());
        }
        // Lazy gaussian init must not change the LCG stream.
        let mut st2 = SingleThreadedRandomSource::new(7);
        st2.next_gaussian();
        let mut l2 = LegacyRandomSource::new(7);
        l2.next_gaussian();
        assert_eq!(st2.next_int(), l2.next_int());
    }

    #[test]
    fn thread_safe_matches_legacy() {
        let mut ts = ThreadSafeLegacyRandomSource::new(12345);
        let mut l = LegacyRandomSource::new(12345);
        for _ in 0..10 {
            assert_eq!(ts.next_int(), l.next_int());
        }
        for _ in 0..10 {
            assert_eq!(ts.next_long(), l.next_long());
        }
    }

    #[test]
    fn create_factories_return_working_sources() {
        // create()/createThreadSafe() seed from generateUniqueSeed() (unique, so
        // only sanity-check that the sources produce in-range draws).
        let mut r = random_source_create();
        for _ in 0..100 {
            assert!((0.0..1.0).contains(&r.next_double()));
            assert!(r.next_int_bound(10) < 10);
        }
        let mut r = random_source_create_thread_safe();
        for _ in 0..100 {
            assert!((0.0..1.0).contains(&r.next_double()));
        }
        let mut r = random_source_create_thread_local_instance();
        for _ in 0..100 {
            assert!((0.0..1.0).contains(&r.next_double()));
        }
    }
}
