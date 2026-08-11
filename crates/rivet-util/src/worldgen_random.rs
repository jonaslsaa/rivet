//! `net.minecraft.world.level.levelgen.WorldgenRandom` — the worldgen seed
//! decorator, plus its `Algorithm` enum and the `RandomSource`/factory
//! wrappers `Algorithm.newInstance` returns.
//!
//! Java hierarchy: `WorldgenRandom extends LegacyRandomSource`. The base LCG
//! seed is dead — both `next(bits)` and `setSeed` are overridden to forward to
//! the wrapped `randomSource` — but the base's `MarsagliaPolarGaussian` is NOT:
//! `nextGaussian()` still uses it, and the overridden `setSeed` never calls
//! `super.setSeed`, so it does not reset that gaussian cache. We model the
//! gaussian cache directly on `WorldgenRandom` (probe-confirmed; see the
//! gaussian counterfactual test).
//!
//! The wrapped source is generic (`S: RandomSource`), mirroring Java's
//! `final RandomSource randomSource` field. Every Java construction site wraps
//! a `LegacyRandomSource` or `XoroshiroRandomSource`; `WorldgenRandom` accepts
//! any `RandomSource` (incl. `SingleThreadedRandomSource`, used by
//! `seedSlimeChunk`).
//!
//! `next(bits)` reproduces Java's `randomSource instanceof LegacyRandomSource`
//! special case via an `Any` downcast: only a wrapped `LegacyRandomSource`
//! takes the direct 48-bit LCG `next(bits)` path; every other source shifts
//! `nextLong()`. (Java's `instanceof` would also match a nested `WorldgenRandom`
//! — it IS a `LegacyRandomSource` — but no real consumer nests one, so that
//! case is not supported here.)

use std::any::Any;

// `BitRandomSource` is deliberately NOT imported here: it declares
// `next_int`/`next_long`/... (same names as `RandomSource`), so importing both
// makes every LCG call on `LegacyRandomSource` ambiguous (E0034). It is
// referenced fully-qualified (`crate::random::BitRandomSource`) in the two
// places it is needed.
use crate::random::{
    LegacyPositionalRandomFactory, LegacyRandomSource, MarsagliaPolarGaussian,
    PositionalRandomFactory, RandomSource, SingleThreadedRandomSource,
    XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
    random_source_create_thread_local_instance_with_seed,
};

/// `net.minecraft.world.level.levelgen.WorldgenRandom`.
pub struct WorldgenRandom<S> {
    inner: S,
    count: i32,
    gaussian_source: MarsagliaPolarGaussian,
}

impl<S: RandomSource + 'static> WorldgenRandom<S> {
    /// `WorldgenRandom(RandomSource)`.
    pub fn new(inner: S) -> Self {
        WorldgenRandom {
            inner,
            count: 0,
            gaussian_source: MarsagliaPolarGaussian::new(),
        }
    }

    /// `WorldgenRandom.getCount()`.
    pub fn get_count(&self) -> i32 {
        self.count
    }

    fn next_gaussian_inner(&mut self) -> f64 {
        // `MarsagliaPolarGaussian::next_gaussian` borrows the source mutably,
        // so the field is temporarily taken out of `self` to satisfy the
        // borrow checker.
        let mut gaussian = std::mem::take(&mut self.gaussian_source);
        let value = gaussian.next_gaussian(self);
        self.gaussian_source = gaussian;
        value
    }

    /// `WorldgenRandom.setDecorationSeed(long seed, int chunkX, int chunkZ)` —
    /// returns the derived seed.
    pub fn set_decoration_seed(&mut self, seed: i64, chunk_x: i32, chunk_z: i32) -> i64 {
        self.set_seed(seed);
        let x_scale = self.next_long() | 1;
        let z_scale = self.next_long() | 1;
        let result = (chunk_x as i64)
            .wrapping_mul(x_scale)
            .wrapping_add((chunk_z as i64).wrapping_mul(z_scale))
            ^ seed;
        self.set_seed(result);
        result
    }

    /// `WorldgenRandom.setFeatureSeed(long seed, int index, int step)`.
    ///
    /// `10000 * step` is INT arithmetic (wraps at 32 bits) before widening.
    pub fn set_feature_seed(&mut self, seed: i64, index: i32, step: i32) {
        let result = seed
            .wrapping_add(index as i64)
            .wrapping_add(10000i32.wrapping_mul(step) as i64);
        self.set_seed(result);
    }

    /// `WorldgenRandom.setLargeFeatureSeed(long seed, int chunkX, int chunkZ)`.
    pub fn set_large_feature_seed(&mut self, seed: i64, chunk_x: i32, chunk_z: i32) {
        self.set_seed(seed);
        let x_scale = self.next_long();
        let z_scale = self.next_long();
        let result =
            (chunk_x as i64).wrapping_mul(x_scale) ^ (chunk_z as i64).wrapping_mul(z_scale) ^ seed;
        self.set_seed(result);
    }

    /// `WorldgenRandom.setLargeFeatureWithSalt(long seed, int x, int z, int
    /// blend)`.
    ///
    /// Paper comment preserved: `// Paper - diff on change for
    /// CustomChunkGenerator`.
    pub fn set_large_feature_with_salt(&mut self, seed: i64, x: i32, z: i32, blend: i32) {
        let result = (x as i64)
            .wrapping_mul(341873128712)
            .wrapping_add((z as i64).wrapping_mul(132897987541))
            .wrapping_add(seed)
            .wrapping_add(blend as i64);
        self.set_seed(result);
    }
}

impl WorldgenRandom<SingleThreadedRandomSource> {
    /// `WorldgenRandom.seedSlimeChunk(int x, int z, long seed, long salt)`.
    ///
    /// Java static (returns a bare thread-local `RandomSource`), so it lives on
    /// the concrete `SingleThreadedRandomSource` variant of the generic rather
    /// than the generic block.
    ///
    /// The additive products are evaluated exactly as in Java: `x*x*4987142`,
    /// `x*5947611` and `z*389711` are INT arithmetic (wrapping); `z*z*4392871L`
    /// is int-by-long (widened); the sum then XORs `salt`.
    pub fn seed_slime_chunk(x: i32, z: i32, seed: i64, salt: i64) -> SingleThreadedRandomSource {
        let mut result = seed;
        result = result.wrapping_add(x.wrapping_mul(x).wrapping_mul(4987142) as i64);
        result = result.wrapping_add(x.wrapping_mul(5947611) as i64);
        result = result.wrapping_add((z.wrapping_mul(z) as i64).wrapping_mul(4392871));
        result = result.wrapping_add(z.wrapping_mul(389711) as i64);
        random_source_create_thread_local_instance_with_seed(result ^ salt)
    }
}

impl<S: RandomSource + 'static> RandomSource for WorldgenRandom<S> {
    type Positional = S::Positional;

    fn fork(&mut self) -> Self {
        // Java returns `this.randomSource.fork()` — the BARE inner source. The
        // `RandomSource::fork -> Self` trait forces a `WorldgenRandom` here
        // instead. This is behaviorally identical for a wrapped
        // `LegacyRandomSource` (both route `nextInt` through the direct LCG
        // `next(bits)` path) and only differs for non-Legacy inners — and no
        // reachable worldgen consumer ever calls `fork()` on a `WorldgenRandom`
        // (the only `RandomSource.fork()` call site forks a bare
        // `RandomSource.create()` Legacy). See the fork counterfactual test.
        WorldgenRandom::new(self.inner.fork())
    }

    fn fork_positional(&mut self) -> Self::Positional {
        self.inner.fork_positional()
    }

    fn set_seed(&mut self, seed: i64) {
        // Java: `if (randomSource != null) randomSource.setSeed(seed);` —
        // deliberately NOT resetting our own gaussian cache (no super.setSeed).
        self.inner.set_seed(seed);
    }

    fn next_int(&mut self) -> i32 {
        <Self as crate::random::BitRandomSource>::next_int(self)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        <Self as crate::random::BitRandomSource>::next_int_bound(self, bound)
    }

    fn next_long(&mut self) -> i64 {
        <Self as crate::random::BitRandomSource>::next_long(self)
    }

    fn next_boolean(&mut self) -> bool {
        <Self as crate::random::BitRandomSource>::next_boolean(self)
    }

    fn next_float(&mut self) -> f32 {
        <Self as crate::random::BitRandomSource>::next_float(self)
    }

    fn next_double(&mut self) -> f64 {
        <Self as crate::random::BitRandomSource>::next_double(self)
    }

    fn next_gaussian(&mut self) -> f64 {
        self.next_gaussian_inner()
    }
}

impl<S: RandomSource + 'static> crate::random::BitRandomSource for WorldgenRandom<S> {
    /// `WorldgenRandom.next(int bits)` — the wrapped-source dispatch.
    fn next(&mut self, bits: i32) -> i32 {
        self.count = self.count.wrapping_add(1);
        // Java: `randomSource instanceof LegacyRandomSource l ? l.next(bits)
        // : (int)(randomSource.nextLong() >>> 64 - bits)`.
        if let Some(legacy) = <dyn Any>::downcast_mut::<LegacyRandomSource>(&mut self.inner) {
            <LegacyRandomSource as crate::random::BitRandomSource>::next(legacy, bits)
        } else {
            (self.inner.next_long() as u64 >> (64 - bits)) as i32
        }
    }
}

/// `WorldgenRandom.Algorithm` — the worldgen RNG selector used by
/// `NoiseGeneratorSettings` / `RandomState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    /// `LEGACY(LegacyRandomSource::new)`.
    Legacy,
    /// `XOROSHIRO(XoroshiroRandomSource::new)`.
    Xoroshiro,
}

impl Algorithm {
    /// `Algorithm.newInstance(long seed)` — a fresh source of the selected
    /// kind. Java returns the bare `RandomSource` (never a `WorldgenRandom`);
    /// Rust returns a sealed enum over the two concrete sources.
    pub fn new_instance(self, seed: i64) -> AlgorithmRandomSource {
        match self {
            Algorithm::Legacy => AlgorithmRandomSource::Legacy(LegacyRandomSource::new(seed)),
            Algorithm::Xoroshiro => {
                AlgorithmRandomSource::Xoroshiro(XoroshiroRandomSource::new(seed))
            }
        }
    }
}

/// The concrete `RandomSource` `Algorithm.newInstance` returns.
pub enum AlgorithmRandomSource {
    Legacy(LegacyRandomSource),
    Xoroshiro(XoroshiroRandomSource),
}

impl RandomSource for AlgorithmRandomSource {
    type Positional = AlgorithmPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        match self {
            AlgorithmRandomSource::Legacy(inner) => AlgorithmRandomSource::Legacy(inner.fork()),
            AlgorithmRandomSource::Xoroshiro(inner) => {
                AlgorithmRandomSource::Xoroshiro(inner.fork())
            }
        }
    }

    fn fork_positional(&mut self) -> Self::Positional {
        match self {
            AlgorithmRandomSource::Legacy(inner) => {
                AlgorithmPositionalRandomFactory::Legacy(inner.fork_positional())
            }
            AlgorithmRandomSource::Xoroshiro(inner) => {
                AlgorithmPositionalRandomFactory::Xoroshiro(inner.fork_positional())
            }
        }
    }

    fn set_seed(&mut self, seed: i64) {
        match self {
            AlgorithmRandomSource::Legacy(inner) => inner.set_seed(seed),
            AlgorithmRandomSource::Xoroshiro(inner) => inner.set_seed(seed),
        }
    }

    fn next_int(&mut self) -> i32 {
        match self {
            AlgorithmRandomSource::Legacy(inner) => inner.next_int(),
            AlgorithmRandomSource::Xoroshiro(inner) => inner.next_int(),
        }
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        match self {
            AlgorithmRandomSource::Legacy(inner) => inner.next_int_bound(bound),
            AlgorithmRandomSource::Xoroshiro(inner) => inner.next_int_bound(bound),
        }
    }

    fn next_long(&mut self) -> i64 {
        match self {
            AlgorithmRandomSource::Legacy(inner) => inner.next_long(),
            AlgorithmRandomSource::Xoroshiro(inner) => inner.next_long(),
        }
    }

    fn next_boolean(&mut self) -> bool {
        match self {
            AlgorithmRandomSource::Legacy(inner) => inner.next_boolean(),
            AlgorithmRandomSource::Xoroshiro(inner) => inner.next_boolean(),
        }
    }

    fn next_float(&mut self) -> f32 {
        match self {
            AlgorithmRandomSource::Legacy(inner) => inner.next_float(),
            AlgorithmRandomSource::Xoroshiro(inner) => inner.next_float(),
        }
    }

    fn next_double(&mut self) -> f64 {
        match self {
            AlgorithmRandomSource::Legacy(inner) => inner.next_double(),
            AlgorithmRandomSource::Xoroshiro(inner) => inner.next_double(),
        }
    }

    fn next_gaussian(&mut self) -> f64 {
        match self {
            AlgorithmRandomSource::Legacy(inner) => inner.next_gaussian(),
            AlgorithmRandomSource::Xoroshiro(inner) => inner.next_gaussian(),
        }
    }
}

/// The `PositionalRandomFactory` the `Algorithm` sources `forkPositional()`
/// into.
#[derive(Clone, Copy, Debug)]
pub enum AlgorithmPositionalRandomFactory {
    Legacy(LegacyPositionalRandomFactory),
    Xoroshiro(XoroshiroPositionalRandomFactory),
}

impl PositionalRandomFactory for AlgorithmPositionalRandomFactory {
    type Output = AlgorithmRandomSource;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Output {
        match self {
            AlgorithmPositionalRandomFactory::Legacy(inner) => {
                AlgorithmRandomSource::Legacy(inner.at(x, y, z))
            }
            AlgorithmPositionalRandomFactory::Xoroshiro(inner) => {
                AlgorithmRandomSource::Xoroshiro(inner.at(x, y, z))
            }
        }
    }

    fn from_hash_of(&self, name: &str) -> Self::Output {
        match self {
            AlgorithmPositionalRandomFactory::Legacy(inner) => {
                AlgorithmRandomSource::Legacy(inner.from_hash_of(name))
            }
            AlgorithmPositionalRandomFactory::Xoroshiro(inner) => {
                AlgorithmRandomSource::Xoroshiro(inner.from_hash_of(name))
            }
        }
    }

    fn from_seed(&self, seed: i64) -> Self::Output {
        match self {
            AlgorithmPositionalRandomFactory::Legacy(inner) => {
                AlgorithmRandomSource::Legacy(inner.from_seed(seed))
            }
            AlgorithmPositionalRandomFactory::Xoroshiro(inner) => {
                AlgorithmRandomSource::Xoroshiro(inner.from_seed(seed))
            }
        }
    }

    fn parity_config_string(&self, sb: &mut String) {
        match self {
            AlgorithmPositionalRandomFactory::Legacy(inner) => inner.parity_config_string(sb),
            AlgorithmPositionalRandomFactory::Xoroshiro(inner) => inner.parity_config_string(sb),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Algorithm, AlgorithmPositionalRandomFactory, AlgorithmRandomSource, WorldgenRandom,
    };
    use crate::random::{
        LegacyRandomSource, PositionalRandomFactory, RandomSource, SingleThreadedRandomSource,
        XoroshiroRandomSource,
    };

    fn legacy_wgr() -> WorldgenRandom<LegacyRandomSource> {
        WorldgenRandom::new(LegacyRandomSource::new(0))
    }

    fn xoroshiro_wgr() -> WorldgenRandom<XoroshiroRandomSource> {
        WorldgenRandom::new(XoroshiroRandomSource::new(0))
    }

    /// `setLargeFeatureSeed` then the next ints, longs, floats, doubles,
    /// gaussians — goldens from the pinned Paper 26.2 runtime probe.
    #[allow(clippy::type_complexity)] // the 5-sequence tuple is test-only and self-documenting
    fn lfs_seq<S: RandomSource + 'static>(
        make: impl Fn() -> WorldgenRandom<S>,
        seed: i64,
    ) -> (Vec<u32>, Vec<i64>, Vec<u32>, Vec<u64>, Vec<u64>) {
        let mut r = make();
        r.set_large_feature_seed(seed, 3, -7);
        let ints: Vec<u32> = (0..5).map(|_| r.next_int() as u32).collect();
        let longs: Vec<i64> = (0..3).map(|_| r.next_long()).collect();
        let floats: Vec<u32> = (0..3).map(|_| r.next_float().to_bits()).collect();
        let doubles: Vec<u64> = (0..3).map(|_| r.next_double().to_bits()).collect();
        let gaussians: Vec<u64> = (0..4).map(|_| r.next_gaussian().to_bits()).collect();
        (ints, longs, floats, doubles, gaussians)
    }

    #[test]
    fn large_feature_seed_legacy_inner_golden() {
        // One instance consumed sequentially (5 int, 3 long, 3 float, 3 double,
        // 4 gaussian). Goldens captured from the pinned Paper runtime via
        // SeqProbe.lfsSeq, which mirrors this exact call order.
        let (ints, longs, floats, doubles, gaussians) = lfs_seq(legacy_wgr, 0);
        assert_eq!(
            ints,
            [2908543734, 917254086, 992506311, 2296573192, 1817499462]
        );
        assert_eq!(
            longs,
            [
                -3008658963283648362,
                2997511708634493405,
                7185379156546063299
            ]
        );
        assert_eq!(floats, [0x3dc3b9f0, 0x3f1d6106, 0x3f04d437]);
        assert_eq!(
            doubles,
            [0x3fdecf4d60000000, 0x3fec32b800000000, 0x3fe02d84c0000000]
        );
        assert_eq!(
            gaussians,
            [
                0xbf9757d83e3623fd,
                0x3fd8baa131002541,
                0x3fb34d3e0b710bae,
                0x3fd892772a36840d
            ]
        );

        let (ints, longs, floats, doubles, gaussians) = lfs_seq(legacy_wgr, 12345);
        assert_eq!(
            ints,
            [2008039510, 1263303674, 49608414, 3921542002, 906578986]
        );
        assert_eq!(
            longs,
            [
                3913051312985718863,
                -6808304852606306666,
                -5482878084288439652
            ]
        );
        assert_eq!(floats, [0x3f668b80, 0x3e8ba268, 0x3f71f03d]);
        assert_eq!(
            doubles,
            [0x3fe9518660000000, 0x3fbe857180000000, 0x3fd451c760000000]
        );
        assert_eq!(
            gaussians,
            [
                0xbfe54acdec28b3d5,
                0x3fe41834e970ea76,
                // 2nd polar pair: Java (SeqProbe) is 0x3fd81061857ba03e /
                // 0xbff68649a1ba5be8; Rust's libm ln is 1 ULP lower here — the
                // pre-existing MarsagliaPolarGaussian gap tracked by
                // RivetTodo(#177) in random.rs.
                0x3fd81061857ba03d,
                0xbff68649a1ba5be7
            ]
        );

        let (ints, longs, floats, doubles, gaussians) = lfs_seq(legacy_wgr, -987654321);
        assert_eq!(
            ints,
            [455090121, 1120961629, 1364273727, 414588248, 714560862]
        );
        assert_eq!(
            longs,
            [
                1447672852089378505,
                -7740969210108661715,
                2606076356020274469
            ]
        );
        assert_eq!(floats, [0x3af20e00, 0x3f7e3c2a, 0x3f05861f]);
        assert_eq!(
            doubles,
            [0x3fb96982c0000000, 0x3fe84c3da0000000, 0x3fe2934ca0000000]
        );
        assert_eq!(
            gaussians,
            [
                0xbfe1a4beed1593de,
                0x3ff3b648fcc5a8fe,
                0x3f97fddcddf14762,
                0xbff42277fbd45bb9
            ]
        );
    }

    #[test]
    fn large_feature_seed_legacy_inner_min_matches_zero() {
        // setSeed masks with & MODULUS_MASK (48 bits), so seed 0 and
        // Long.MIN_VALUE produce identical LCG state and identical sequences.
        assert_eq!(lfs_seq(legacy_wgr, 0), lfs_seq(legacy_wgr, i64::MIN));
    }

    #[test]
    fn large_feature_seed_xoroshiro_inner_golden() {
        // Sequential single-instance consumption; goldens from SeqProbe.lfsSeq.
        let (ints, longs, floats, doubles, gaussians) = lfs_seq(xoroshiro_wgr, 0);
        assert_eq!(
            ints,
            [3066458297, 155487874, 2668911164, 1831149122, 471499684]
        );
        assert_eq!(
            longs,
            [
                -8452608508603117296,
                -650097665183500656,
                1336792990875184606
            ]
        );
        assert_eq!(floats, [0x3f09bab0, 0x3f75879a, 0x3f6c896c]);
        assert_eq!(
            doubles,
            [0x3fcb88a400000000, 0x3fd5b5c140000000, 0x3fd2516940000000]
        );
        assert_eq!(
            gaussians,
            [
                // Each is Java (SeqProbe) - 1 ULP, except the 3rd which matches:
                //   Java  bfef1fb3550defeb bffeef0d089f340f bfe30c5e2644c840 bfca7b41e262922f
                // (RivetTodo #177 in random.rs: libm ln/sqrt vs fdlibm Math.log).
                0xbfef1fb3550defec,
                0xbffeef0d089f3410,
                0xbfe30c5e2644c840,
                0xbfca7b41e2629230
            ]
        );

        let (ints, longs, floats, doubles, gaussians) = lfs_seq(xoroshiro_wgr, 12345);
        assert_eq!(
            ints,
            [112461062, 3630298534, 1752296736, 3236513647, 3083382849]
        );
        assert_eq!(
            longs,
            [
                -8684787154082827934,
                475028262772736921,
                -1618963102374549376
            ]
        );
        assert_eq!(floats, [0x3f34379b, 0x3d2702f0, 0x3ed39cb8]);
        assert_eq!(
            doubles,
            [0x3fdfc420a0000000, 0x3fb8369ec0000000, 0x3fc544ed40000000]
        );
        assert_eq!(
            gaussians,
            [
                0x3fe83d60573a4d92,
                0x3fc07c762f627b40,
                0xbfddd5168e9c17e3,
                0xbf6406825f5218e3
            ]
        );

        let (ints, longs, floats, doubles, gaussians) = lfs_seq(xoroshiro_wgr, i64::MIN);
        assert_eq!(
            ints,
            [16640480, 1067293864, 3326990623, 3030791420, 1725852307]
        );
        assert_eq!(
            longs,
            [
                -6274685890181373087,
                4930264076833074285,
                1004292235351089367
            ]
        );
        assert_eq!(floats, [0x3b581a00, 0x3dd20258, 0x3f57b8a9]);
        assert_eq!(
            doubles,
            [0x3fd9fa8d20000000, 0x3fc5bfbbe0000000, 0x3fbc046800000000]
        );
        assert_eq!(
            gaussians,
            [
                0x3fd9cf27af663aa0,
                0x3fa393ce0aa38ef8,
                0x3ffebedf0ec59b4b,
                0xbfea5987784913cc
            ]
        );
    }

    #[test]
    fn large_feature_seed_single_threaded_inner_uses_else_branch() {
        // Java's next(bits) special-cases ONLY `instanceof LegacyRandomSource`;
        // SingleThreadedRandomSource (a non-Legacy BitRandomSource) must take the
        // `nextLong() >>> (64 - bits)` branch. Goldens from the pinned Paper.
        let mut r = WorldgenRandom::new(SingleThreadedRandomSource::new(0));
        r.set_large_feature_seed(0, 3, -7);
        let ints: Vec<u32> = (0..5).map(|_| r.next_int() as u32).collect();
        assert_eq!(
            ints,
            [909703280, 2159176223, 565090644, 325083738, 1913123917]
        );

        let mut r = WorldgenRandom::new(SingleThreadedRandomSource::new(0));
        r.set_large_feature_seed(12345, 3, -7);
        let ints: Vec<u32> = (0..5).map(|_| r.next_int() as u32).collect();
        assert_eq!(
            ints,
            [3784091373, 3720251369, 1766581338, 3382588683, 3398850722]
        );
    }

    #[test]
    fn count_tracks_next_calls() {
        let mut r = legacy_wgr();
        r.set_large_feature_seed(12345, 3, -7);
        assert_eq!(r.get_count(), 4, "2 x nextLong");
        r.next_int();
        r.next_int_bound(1000);
        r.next_float();
        r.next_long();
        assert_eq!(r.get_count(), 9);

        let mut x = xoroshiro_wgr();
        x.set_large_feature_seed(12345, 3, -7);
        assert_eq!(x.get_count(), 4);
        x.next_int();
        assert_eq!(x.get_count(), 5);

        let mut g = legacy_wgr();
        g.set_large_feature_seed(12345, 3, -7);
        g.next_gaussian();
        assert_eq!(
            g.get_count(),
            8,
            "fresh gaussian draws 2 next-doubles = 4 next-bits"
        );
        g.next_gaussian();
        assert_eq!(g.get_count(), 8, "stored gaussian draws nothing");
    }

    #[test]
    fn set_decoration_seed_golden() {
        let mut r = legacy_wgr();
        assert_eq!(r.set_decoration_seed(12345, 3, -7), -8218855382820530819);
        let ints: Vec<u32> = (0..5).map(|_| r.next_int() as u32).collect();
        assert_eq!(
            ints,
            [833649654, 26367617, 2216965994, 3976741307, 3693605589]
        );

        let mut r = xoroshiro_wgr();
        assert_eq!(r.set_decoration_seed(12345, 3, -7), 1978491184174722315);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [2664844228, 3559218574, 120401429]);

        let mut r = legacy_wgr();
        assert_eq!(r.set_decoration_seed(i64::MAX, 3, -7), -4075587773116017393);
        let ints: Vec<u32> = (0..5).map(|_| r.next_int() as u32).collect();
        assert_eq!(
            ints,
            [1083145821, 2469502849, 2081839620, 4286919463, 1364111469]
        );

        let mut r = xoroshiro_wgr();
        assert_eq!(r.set_decoration_seed(i64::MAX, 3, -7), -6301519507305034369);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [2310188788, 1758388490, 2502004104]);
    }

    #[test]
    fn set_large_feature_with_salt_golden() {
        let mut r = legacy_wgr();
        r.set_large_feature_with_salt(12345, 3, -7, 9);
        let ints: Vec<u32> = (0..5).map(|_| r.next_int() as u32).collect();
        assert_eq!(
            ints,
            [2266330732, 287355851, 3024399162, 695508285, 2568970648]
        );

        let mut r = xoroshiro_wgr();
        r.set_large_feature_with_salt(12345, 3, -7, 9);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [998287908, 3810164121, 2380186541]);

        // x/z large enough that `x * 341873128712L` overflows the long.
        let mut r = legacy_wgr();
        r.set_large_feature_with_salt(12345, -1234567, 891011, -42);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [2123462740, 4218011136, 3111478933]);
    }

    #[test]
    fn set_feature_seed_golden() {
        let mut r = legacy_wgr();
        r.set_feature_seed(12345, 3, 7);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [706638803, 3817924757, 641134927]);

        // 10000 * step wraps the i32 before widening.
        let mut r = legacy_wgr();
        r.set_feature_seed(12345, 3, 10_000_000);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [496957332, 1490928760, 1994193785]);
    }

    #[test]
    fn algorithm_new_instance_golden() {
        // LEGACY.newInstance(seed).forkPositional().at(1,2,3)
        let mut r = Algorithm::Legacy
            .new_instance(12345)
            .fork_positional()
            .at(1, 2, 3);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [2748230745, 87825912, 90915984]);

        let mut r = Algorithm::Xoroshiro
            .new_instance(12345)
            .fork_positional()
            .at(1, 2, 3);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [118058776, 2221092260, 1727080943]);

        let mut r = Algorithm::Xoroshiro
            .new_instance(12345)
            .fork_positional()
            .from_seed(999);
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [1053988486, 3916092551, 1407022417]);

        let mut r = Algorithm::Legacy
            .new_instance(12345)
            .fork_positional()
            .from_hash_of("minecraft:overworld");
        let ints: Vec<u32> = (0..3).map(|_| r.next_int() as u32).collect();
        assert_eq!(ints, [318307362, 3746913505, 1415610748]);
    }

    #[test]
    fn algorithm_new_instance_parity_config_string() {
        let mut sb = String::new();
        AlgorithmPositionalRandomFactory::Legacy(
            crate::random::LegacyPositionalRandomFactory::new(99),
        )
        .parity_config_string(&mut sb);
        assert_eq!(sb, "LegacyPositionalRandomFactory{99}");
        let mut sb = String::new();
        AlgorithmPositionalRandomFactory::Xoroshiro(
            crate::random::XoroshiroPositionalRandomFactory::new(99, 1234),
        )
        .parity_config_string(&mut sb);
        assert_eq!(sb, "seedLo: 99, seedHi: 1234");
    }

    #[test]
    fn algorithm_random_source_delegates() {
        let mut src = AlgorithmRandomSource::Legacy(LegacyRandomSource::new(12345));
        assert_eq!(src.next_long(), LegacyRandomSource::new(12345).next_long());
        src.set_seed(7);
        assert_eq!(src.next_int(), LegacyRandomSource::new(7).next_int());
        let f = src.fork_positional();
        assert_eq!(
            f.from_seed(42).next_int(),
            crate::random::LegacyPositionalRandomFactory::new(7)
                .from_seed(42)
                .next_int()
        );
    }

    #[test]
    fn seed_slime_chunk_golden() {
        let mut r = WorldgenRandom::seed_slime_chunk(3, -7, 12345, 999);
        let ints: Vec<u32> = (0..5).map(|_| r.next_int_bound(10) as u32).collect();
        assert_eq!(ints, [9, 1, 8, 2, 8]);
    }

    #[test]
    fn worldgen_random_set_seed_does_not_reset_gaussian() {
        // Counterfactual: WorldgenRandom.setSeed forwards to the wrapped source
        // but never touches its OWN gaussian cache (Java's override does not
        // call super.setSeed). After a fresh gaussian stash, setSeed then
        // nextGaussian returns the stored value.
        let mut w = legacy_wgr();
        w.set_large_feature_seed(12345, 3, -7);
        assert_eq!(w.next_gaussian().to_bits(), 0xbf93c30a58865fb3);
        w.set_seed(12345);
        assert_eq!(
            w.next_gaussian().to_bits(),
            0xbfd294f0abca2aee,
            "stored polar second-half survives setSeed"
        );

        // A bare LegacyRandomSource DOES reset its gaussian on setSeed.
        let mut l = LegacyRandomSource::new(12345);
        assert_eq!(l.next_gaussian().to_bits(), 0xbfc80a203d6615bd);
        assert_eq!(l.next_gaussian().to_bits(), 0x3fe2d47897dd317e);
        l.set_seed(12345);
        assert_eq!(
            l.next_gaussian().to_bits(),
            0xbfc80a203d6615bd,
            "bare Legacy resets the stored value"
        );
    }

    #[test]
    fn fork_golden() {
        // Legacy inner: Java `WorldgenRandom.fork()` returns the bare
        // `LegacyRandomSource.fork()`, whose `nextInt` is the direct LCG
        // `next(32)`. Our wrapped fork routes through the same `instanceof`
        // path, so both produce these identical ints (SeqProbe `fork_legacy`).
        let mut fr = legacy_wgr();
        fr.set_large_feature_seed(12345, 3, -7);
        let mut fork = fr.fork();
        let ints: Vec<u32> = (0..3).map(|_| fork.next_int() as u32).collect();
        assert_eq!(ints, [3367124012, 2974449110, 2392242795]);
    }

    #[test]
    fn fork_xoroshiro_divergence_counterfactual() {
        // Java `WorldgenRandom.fork()` returns the BARE Xoroshiro source, whose
        // `nextInt()` is `(int)generator.nextLong()` (the LOW 32 bits):
        //   [581419728, 1879620837, 465925744]   (SeqProbe `fork_xor`)
        // The Rust `RandomSource::fork -> Self` trait forces our fork to wrap
        // the inner in a `WorldgenRandom`, whose `next()` takes the else-branch
        // `(int)(nextLong() >>> 32)` (the HIGH 32 bits):
        //   [4228850308, 2351048541, 4102331771] (SeqProbe `fork_xor_wrapped_top32`)
        // No reachable worldgen consumer forks a `WorldgenRandom` (the only
        // `RandomSource.fork()` call site forks a bare `RandomSource.create()`
        // Legacy), so the divergence is latent. This test pins our behavior so
        // the divergence is visible if it is ever reached.
        let mut fr = xoroshiro_wgr();
        fr.set_large_feature_seed(12345, 3, -7);
        let mut fork = fr.fork();
        let ints: Vec<u32> = (0..3).map(|_| fork.next_int() as u32).collect();
        assert_eq!(ints, [4228850308, 2351048541, 4102331771]);
    }

    #[test]
    fn legacy_inner_uses_lcg_next_directly() {
        // The instanceof special case must actually take LegacyRandomSource's
        // direct `next(bits)` path: after setLargeFeatureSeed, a wrapped Legacy
        // and a bare Legacy seeded with the same value draw identically.
        let mut bare = LegacyRandomSource::new(0);
        // Mirror set_large_feature_seed's steps to recover the final seed.
        bare.set_seed(12345);
        let x_scale = bare.next_long();
        let z_scale = bare.next_long();
        let final_seed = 3i64.wrapping_mul(x_scale) ^ (-7i64).wrapping_mul(z_scale) ^ 12345;

        let mut wrapped = legacy_wgr();
        wrapped.set_large_feature_seed(12345, 3, -7);
        let mut bare2 = LegacyRandomSource::new(final_seed);
        for _ in 0..20 {
            assert_eq!(
                wrapped.next_int(),
                bare2.next_int(),
                "wrapped Legacy must consume the same LCG stream"
            );
        }
    }
}
