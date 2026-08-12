//! Port of `net.minecraft.world.level.levelgen.synth.SimplexNoise` (class,
//! 26.2).
//!
//! 2D/3D simplex noise over a shuffled 256-entry permutation `p`. Java
//! declares `int[] p = new int[512]` but fills only `[0, 256)` and reads only
//! `p[x & 0xFF]`, so the effective permutation is 256 entries; the Rust port
//! stores exactly that (`[u8; 256]`). The constructor consumes exactly
//! `3 * nextDouble()` + the Fisher-Yates `256 + 255 + ... + 1` `nextInt(bound)`
//! calls from the given `RandomSource`; both the xoroshiro and legacy sources
//! in `rivet-util::random` are bit-exact, so the permutation and origin offsets
//! reproduce exactly.
//!
//! Exactness notes (from the Java):
//! - `getValue` gradient indices are `p[..] % 12` — Java's `%` on a non-negative
//!   index; the permuted values are in `[0, 255]`, so the modulo is exact.
//! - `dot` is `g[0] * x + g[1] * y + g[2] * z` with the same left-to-right
//!   associativity (f64 multiply/add order is observable in the last ULP).
//! - `getCornerNoise3D` does `t0 *= t0` then `t0 * t0 * dot` — the squaring is
//!   a single round of `t0²`, then the t⁴ = (t0²)² is a second round.
//!
//! The parity-visible `xo`/`yo`/`zo` (each `nextDouble() * 256.0`) and the
//! permutation `p` (read via `perm(p, i)` in the probe) are part of the golden
//! fixture.

use rivet_util::mth;
use rivet_util::random::RandomSource;

/// `SimplexNoise.GRADIENT` — the 16 gradient vectors (first 12 are the simplex
/// gradient set; 13–16 repeat entries so `% 12` is a no-op for `p & 15` in
/// `ImprovedNoise`). `pub(crate)` because `ImprovedNoise` reads it directly.
pub(crate) const GRADIENT: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

const SQRT_3: f64 = 1.7320508075688772;
const F2: f64 = 0.5 * (SQRT_3 - 1.0);
const G2: f64 = (3.0 - SQRT_3) / 6.0;

/// `net.minecraft.world.level.levelgen.synth.SimplexNoise`.
#[derive(Debug, Clone)]
pub struct SimplexNoise {
    /// The 256-entry permutation (Java's `int[512]` fills/reads only `[0,256)`).
    p: [u8; 256],
    /// `xo` — the x-origin offset (`nextDouble() * 256.0`).
    pub xo: f64,
    /// `yo` — the y-origin offset.
    pub yo: f64,
    /// `zo` — the z-origin offset.
    pub zo: f64,
}

impl SimplexNoise {
    /// `new SimplexNoise(RandomSource)`.
    pub fn new(random: &mut impl RandomSource) -> Self {
        let xo = random.next_double() * 256.0;
        let yo = random.next_double() * 256.0;
        let zo = random.next_double() * 256.0;
        let mut p = [0u8; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as u8;
        }
        // Fisher-Yates, exactly as Java: `offset = nextInt(256 - ix)`, swap
        // `p[ix]` and `p[offset + ix]`.
        for ix in 0..256usize {
            let offset = random.next_int_bound((256 - ix) as i32);
            let j = (ix as i32 + offset) as usize;
            p.swap(ix, j);
        }
        SimplexNoise { p, xo, yo, zo }
    }

    /// `p(int x)` — `this.p[x & 0xFF]`.
    fn p(&self, x: i32) -> u8 {
        self.p[(x & 0xFF) as usize]
    }

    /// Construction-parity accessor — reads raw permutation entry `index`
    /// (mirrors the oracle probe's reflective `perm(noise, index)`).
    pub fn perm(&self, index: usize) -> u8 {
        self.p[index]
    }

    /// `SimplexNoise.dot(int[] g, double x, double y, double z)`.
    fn dot(g: &[i32; 3], x: f64, y: f64, z: f64) -> f64 {
        g[0] as f64 * x + g[1] as f64 * y + g[2] as f64 * z
    }

    /// `getCornerNoise3D(int index, double x, double y, double z, double base)`.
    fn get_corner_noise_3d(&self, index: usize, x: f64, y: f64, z: f64, base: f64) -> f64 {
        let t0 = base - x * x - y * y - z * z;
        if t0 < 0.0 {
            0.0
        } else {
            let t0_sq = t0 * t0;
            t0_sq * t0_sq * Self::dot(&GRADIENT[index], x, y, z)
        }
    }

    /// `getValue(double xin, double yin)` — 2D simplex.
    pub fn get_value_2d(&self, xin: f64, yin: f64) -> f64 {
        let s = (xin + yin) * F2;
        let i = mth::floor_d(xin + s);
        let j = mth::floor_d(yin + s);
        // Java `i + j` is int arithmetic that wraps (hostile coordinates like
        // `Double.MAX_VALUE / 1e9` saturate `Mth.floor` to `i32::MAX`, whose
        // sum wraps).
        let t = i.wrapping_add(j) as f64 * G2;
        let x0 = xin - (i as f64 - t);
        let y0 = yin - (j as f64 - t);
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
        let x1 = x0 - i1 as f64 + G2;
        let y1 = y0 - j1 as f64 + G2;
        let x2 = x0 - 1.0 + 2.0 * G2;
        let y2 = y0 - 1.0 + 2.0 * G2;
        let ii = i & 0xFF;
        let jj = j & 0xFF;
        let gi0 = (self.p(ii + self.p(jj) as i32) % 12) as usize;
        let gi1 = (self.p(ii + i1 + self.p(jj + j1) as i32) % 12) as usize;
        let gi2 = (self.p(ii + 1 + self.p(jj + 1) as i32) % 12) as usize;
        let n0 = self.get_corner_noise_3d(gi0, x0, y0, 0.0, 0.5);
        let n1 = self.get_corner_noise_3d(gi1, x1, y1, 0.0, 0.5);
        let n2 = self.get_corner_noise_3d(gi2, x2, y2, 0.0, 0.5);
        70.0 * (n0 + n1 + n2)
    }

    /// `getValue(double xin, double yin, double zin)` — 3D simplex.
    pub fn get_value_3d(&self, xin: f64, yin: f64, zin: f64) -> f64 {
        let s = (xin + yin + zin) * 0.3333333333333333;
        let i = mth::floor_d(xin + s);
        let j = mth::floor_d(yin + s);
        let k = mth::floor_d(zin + s);
        // Java `i + j + k` is int arithmetic that wraps (see get_value_2d).
        let t = i.wrapping_add(j).wrapping_add(k) as f64 * 0.16666666666666666;
        let x0 = xin - (i as f64 - t);
        let y0 = yin - (j as f64 - t);
        let z0 = zin - (k as f64 - t);
        let (i1, j1, k1, i2, j2, k2) = if x0 >= y0 {
            if y0 >= z0 {
                (1, 0, 0, 1, 1, 0)
            } else if x0 >= z0 {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if y0 < z0 {
            (0, 0, 1, 0, 1, 1)
        } else if x0 < z0 {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };
        let x1 = x0 - i1 as f64 + 0.16666666666666666;
        let y1 = y0 - j1 as f64 + 0.16666666666666666;
        let z1 = z0 - k1 as f64 + 0.16666666666666666;
        let x2 = x0 - i2 as f64 + 0.3333333333333333;
        let y2 = y0 - j2 as f64 + 0.3333333333333333;
        let z2 = z0 - k2 as f64 + 0.3333333333333333;
        let x3 = x0 - 1.0 + 0.5;
        let y3 = y0 - 1.0 + 0.5;
        let z3 = z0 - 1.0 + 0.5;
        let ii = i & 0xFF;
        let jj = j & 0xFF;
        let kk = k & 0xFF;
        let gi0 = (self.p(ii + self.p(jj + self.p(kk) as i32) as i32) % 12) as usize;
        let gi1 = (self.p(ii + i1 + self.p(jj + j1 + self.p(kk + k1) as i32) as i32) % 12) as usize;
        let gi2 = (self.p(ii + i2 + self.p(jj + j2 + self.p(kk + k2) as i32) as i32) % 12) as usize;
        let gi3 = (self.p(ii + 1 + self.p(jj + 1 + self.p(kk + 1) as i32) as i32) % 12) as usize;
        let n0 = self.get_corner_noise_3d(gi0, x0, y0, z0, 0.6);
        let n1 = self.get_corner_noise_3d(gi1, x1, y1, z1, 0.6);
        let n2 = self.get_corner_noise_3d(gi2, x2, y2, z2, 0.6);
        let n3 = self.get_corner_noise_3d(gi3, x3, y3, z3, 0.6);
        32.0 * (n0 + n1 + n2 + n3)
    }
}
