//! Port of `net.minecraft.world.level.levelgen.synth.ImprovedNoise` (class,
//! 26.2).
//!
//! 3D Perlin (gradient) noise over a byte permutation `p` (256 entries), with
//! the `noise(x, y, z)` / deprecated `noise(x, y, z, yScale, yFudge)` /
//! `noiseWithDerivative` surfaces. `sampleAndLerp` uses `Mth.smoothstep` and
//! `Mth.lerp3` exactly as Java; `sampleWithDerivative` accumulates the
//! derivative into an out-array (the probe records `vDeriv`/`d0`/`d1`/`d2`).
//!
//! The permutation is stored as Java `byte[]` (wrapping to `[-128, 127]`), so
//! `p(x) = p[x & 0xFF] & 0xFF` reproduces the sign-extension. The probe's
//! `perm()` reads raw `byte` values (fixture `p0`/`p255` are signed).

use rivet_util::mth;
use rivet_util::random::RandomSource;

use crate::levelgen::synth::simplex_noise::GRADIENT;

/// `ImprovedNoise.SHIFT_UP_EPSILON` — the `1.0E-7F` fudge-limit epsilon in the
/// deprecated `noise` overload.
const SHIFT_UP_EPSILON: f32 = 1.0e-7;

/// `net.minecraft.world.level.levelgen.synth.ImprovedNoise`.
pub struct ImprovedNoise {
    /// The 256-entry permutation as Java `byte` values.
    p: [i8; 256],
    /// `xo` — `nextDouble() * 256.0`.
    pub xo: f64,
    /// `yo`.
    pub yo: f64,
    /// `zo`.
    pub zo: f64,
}

impl ImprovedNoise {
    /// `new ImprovedNoise(RandomSource)`.
    pub fn new(random: &mut impl RandomSource) -> Self {
        let xo = random.next_double() * 256.0;
        let yo = random.next_double() * 256.0;
        let zo = random.next_double() * 256.0;
        let mut p = [0i8; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as i8;
        }
        // Fisher-Yates; `offset = nextInt(256 - i)`; swap `p[i]`, `p[i +
        // offset]` (Java swaps `this.p[i]` and `this.p[i + offset]`).
        for i in 0..256usize {
            let offset = random.next_int_bound((256 - i) as i32);
            let j = (i as i32 + offset) as usize;
            p.swap(i, j);
        }
        ImprovedNoise { p, xo, yo, zo }
    }

    /// `noise(double x, double y, double z)`.
    pub fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise_yscaled(x, y, z, 0.0, 0.0)
    }

    /// `noise(double x, double y, double z, double yScale, double yFudge)` — the
    /// deprecated overload with the `yrFudge` smoothing.
    pub fn noise_yscaled(&self, x: f64, y: f64, z: f64, y_scale: f64, y_fudge: f64) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;
        let xf = mth::floor_d(x);
        let yf = mth::floor_d(y);
        let zf = mth::floor_d(z);
        let xr = x - xf as f64;
        let yr = y - yf as f64;
        let zr = z - zf as f64;
        let yr_fudge = if y_scale != 0.0 {
            let fudge_limit = if y_fudge >= 0.0 && y_fudge < yr {
                y_fudge
            } else {
                yr
            };
            mth::floor_d(fudge_limit / y_scale + SHIFT_UP_EPSILON as f64) as f64 * y_scale
        } else {
            0.0
        };
        self.sample_and_lerp(xf, yf, zf, xr, yr - yr_fudge, zr, yr)
    }

    /// `noiseWithDerivative(double x, double y, double z, double[] derivativeOut)`.
    ///
    /// Accumulates the derivative into `derivative_out` (the Java method adds
    /// to the out-array, so callers may seed it and accumulate).
    pub fn noise_with_derivative(
        &self,
        x: f64,
        y: f64,
        z: f64,
        derivative_out: &mut [f64; 3],
    ) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;
        let xf = mth::floor_d(x);
        let yf = mth::floor_d(y);
        let zf = mth::floor_d(z);
        let xr = x - xf as f64;
        let yr = y - yf as f64;
        let zr = z - zf as f64;
        self.sample_with_derivative(xf, yf, zf, xr, yr, zr, derivative_out)
    }

    /// `gradDot(int hash, double x, double y, double z)`.
    fn grad_dot(hash: i32, x: f64, y: f64, z: f64) -> f64 {
        let g = &GRADIENT[(hash & 15) as usize];
        g[0] as f64 * x + g[1] as f64 * y + g[2] as f64 * z
    }

    /// `p(int x)` — `this.p[x & 0xFF] & 0xFF`.
    fn p(&self, x: i32) -> i32 {
        self.p[(x & 0xFF) as usize] as i32 & 0xFF
    }

    /// Construction-parity accessor — reads raw permutation entry `index` as
    /// the Java `byte` value (mirrors the oracle probe's reflective
    /// `perm(noise, index)`, which reads the `byte[]` directly).
    pub fn perm(&self, index: usize) -> i8 {
        self.p[index]
    }

    /// `parityConfigString(StringBuilder)` — the `@VisibleForTesting` config
    /// string (used by the parity dump chain in `PerlinNoise`/`NormalNoise`/
    /// `BlendedNoise`).
    pub fn parity_config_string(&self) -> String {
        let mut sb = String::new();
        crate::levelgen::synth::noise_utils::parity_noise_octave_config_string(
            &mut sb, self.xo, self.yo, self.zo, &self.p,
        );
        sb
    }

    /// `sampleAndLerp(...)` — the non-derivative sampling path. The arity
    /// mirrors Java's `sampleAndLerp(int, int, int, double, double, double,
    /// double)` exactly.
    #[allow(clippy::too_many_arguments)]
    fn sample_and_lerp(
        &self,
        x: i32,
        y: i32,
        z: i32,
        xr: f64,
        yr: f64,
        zr: f64,
        yr_original: f64,
    ) -> f64 {
        let x0 = self.p(x);
        let x1 = self.p(x + 1);
        let xy00 = self.p(x0 + y);
        let xy01 = self.p(x0 + y + 1);
        let xy10 = self.p(x1 + y);
        let xy11 = self.p(x1 + y + 1);
        let d000 = Self::grad_dot(self.p(xy00 + z), xr, yr, zr);
        let d100 = Self::grad_dot(self.p(xy10 + z), xr - 1.0, yr, zr);
        let d010 = Self::grad_dot(self.p(xy01 + z), xr, yr - 1.0, zr);
        let d110 = Self::grad_dot(self.p(xy11 + z), xr - 1.0, yr - 1.0, zr);
        let d001 = Self::grad_dot(self.p(xy00 + z + 1), xr, yr, zr - 1.0);
        let d101 = Self::grad_dot(self.p(xy10 + z + 1), xr - 1.0, yr, zr - 1.0);
        let d011 = Self::grad_dot(self.p(xy01 + z + 1), xr, yr - 1.0, zr - 1.0);
        let d111 = Self::grad_dot(self.p(xy11 + z + 1), xr - 1.0, yr - 1.0, zr - 1.0);
        let x_alpha = mth::smoothstep(xr);
        let y_alpha = mth::smoothstep(yr_original);
        let z_alpha = mth::smoothstep(zr);
        mth::lerp3(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }

    /// `sampleWithDerivative(...)` — accumulates the gradient derivative into
    /// `derivative_out` and returns the lerped value. The arity mirrors Java's
    /// `sampleWithDerivative(int, int, int, double, double, double, double[])`.
    #[allow(clippy::too_many_arguments)]
    fn sample_with_derivative(
        &self,
        x: i32,
        y: i32,
        z: i32,
        xr: f64,
        yr: f64,
        zr: f64,
        derivative_out: &mut [f64; 3],
    ) -> f64 {
        let x0 = self.p(x);
        let x1 = self.p(x + 1);
        let xy00 = self.p(x0 + y);
        let xy01 = self.p(x0 + y + 1);
        let xy10 = self.p(x1 + y);
        let xy11 = self.p(x1 + y + 1);
        let p000 = self.p(xy00 + z);
        let p100 = self.p(xy10 + z);
        let p010 = self.p(xy01 + z);
        let p110 = self.p(xy11 + z);
        let p001 = self.p(xy00 + z + 1);
        let p101 = self.p(xy10 + z + 1);
        let p011 = self.p(xy01 + z + 1);
        let p111 = self.p(xy11 + z + 1);
        let g000 = &GRADIENT[(p000 & 15) as usize];
        let g100 = &GRADIENT[(p100 & 15) as usize];
        let g010 = &GRADIENT[(p010 & 15) as usize];
        let g110 = &GRADIENT[(p110 & 15) as usize];
        let g001 = &GRADIENT[(p001 & 15) as usize];
        let g101 = &GRADIENT[(p101 & 15) as usize];
        let g011 = &GRADIENT[(p011 & 15) as usize];
        let g111 = &GRADIENT[(p111 & 15) as usize];
        let d000 = g000[0] as f64 * xr + g000[1] as f64 * yr + g000[2] as f64 * zr;
        let d100 = g100[0] as f64 * (xr - 1.0) + g100[1] as f64 * yr + g100[2] as f64 * zr;
        let d010 = g010[0] as f64 * xr + g010[1] as f64 * (yr - 1.0) + g010[2] as f64 * zr;
        let d110 = g110[0] as f64 * (xr - 1.0) + g110[1] as f64 * (yr - 1.0) + g110[2] as f64 * zr;
        let d001 = g001[0] as f64 * xr + g001[1] as f64 * yr + g001[2] as f64 * (zr - 1.0);
        let d101 = g101[0] as f64 * (xr - 1.0) + g101[1] as f64 * yr + g101[2] as f64 * (zr - 1.0);
        let d011 = g011[0] as f64 * xr + g011[1] as f64 * (yr - 1.0) + g011[2] as f64 * (zr - 1.0);
        let d111 =
            g111[0] as f64 * (xr - 1.0) + g111[1] as f64 * (yr - 1.0) + g111[2] as f64 * (zr - 1.0);
        let x_alpha = mth::smoothstep(xr);
        let y_alpha = mth::smoothstep(yr);
        let z_alpha = mth::smoothstep(zr);
        let d1x = mth::lerp3(
            x_alpha,
            y_alpha,
            z_alpha,
            g000[0] as f64,
            g100[0] as f64,
            g010[0] as f64,
            g110[0] as f64,
            g001[0] as f64,
            g101[0] as f64,
            g011[0] as f64,
            g111[0] as f64,
        );
        let d1y = mth::lerp3(
            x_alpha,
            y_alpha,
            z_alpha,
            g000[1] as f64,
            g100[1] as f64,
            g010[1] as f64,
            g110[1] as f64,
            g001[1] as f64,
            g101[1] as f64,
            g011[1] as f64,
            g111[1] as f64,
        );
        let d1z = mth::lerp3(
            x_alpha,
            y_alpha,
            z_alpha,
            g000[2] as f64,
            g100[2] as f64,
            g010[2] as f64,
            g110[2] as f64,
            g001[2] as f64,
            g101[2] as f64,
            g011[2] as f64,
            g111[2] as f64,
        );
        let d2x = mth::lerp2(
            y_alpha,
            z_alpha,
            d100 - d000,
            d110 - d010,
            d101 - d001,
            d111 - d011,
        );
        let d2y = mth::lerp2(
            z_alpha,
            x_alpha,
            d010 - d000,
            d011 - d001,
            d110 - d100,
            d111 - d101,
        );
        let d2z = mth::lerp2(
            x_alpha,
            y_alpha,
            d001 - d000,
            d101 - d100,
            d011 - d010,
            d111 - d110,
        );
        let x_sd = mth::smoothstep_derivative(xr);
        let y_sd = mth::smoothstep_derivative(yr);
        let z_sd = mth::smoothstep_derivative(zr);
        derivative_out[0] += d1x + x_sd * d2x;
        derivative_out[1] += d1y + y_sd * d2y;
        derivative_out[2] += d1z + z_sd * d2z;
        mth::lerp3(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }
}
