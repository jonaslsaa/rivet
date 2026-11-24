//! `java.lang.Math` transcendentals that the JDK delegates to `StrictMath`
//! (fdlibm 5.3), ported from the same algorithms: `Math.atan2`, `Math.asin`
//! and the `atan` that `atan2` calls.
//!
//! HotSpot intrinsifies only `sin`/`cos`/`tan`/`exp`/`log`/`log10`/`pow`; the
//! functions here have no intrinsic, so `Math.atan2(y, x)` is
//! `StrictMath.atan2(y, x)` is fdlibm `__ieee754_atan2`. Rust's `f64::atan2`
//! calls the platform libm and can differ in the last bit, so game code that
//! ports `Math.atan2`/`Math.asin` must use these (PORTING.md: Java `Math`
//! semantics).

// fdlibm's literals are the exact published coefficients; they are not meant to be `consts::PI`.
#![allow(clippy::approx_constant)]

#[allow(clippy::excessive_precision)]
mod fdlibm_constants {
    pub const ATAN_HI: [f64; 4] = [
        4.63647609000806093515e-01,
        7.85398163397448278999e-01,
        9.82793723247329054082e-01,
        1.57079632679489655800e+00,
    ];
    pub const ATAN_LO: [f64; 4] = [
        2.26987774529616870924e-17,
        3.06161699786838301793e-17,
        1.39033110312309984516e-17,
        6.12323399573676603587e-17,
    ];
    pub const A_T: [f64; 11] = [
        3.33333333333329318027e-01,
        -1.99999999998764832476e-01,
        1.42857142725034663711e-01,
        -1.11111104054623557880e-01,
        9.09088713343650656196e-02,
        -7.69187620504482999495e-02,
        6.66107313738753120669e-02,
        -5.83357013379057348645e-02,
        4.97687799461593236017e-02,
        -3.65315727442169155270e-02,
        1.62858201153657823623e-02,
    ];
    pub const PI_O_4: f64 = 7.8539816339744827900E-01;
    pub const PI_O_2: f64 = 1.5707963267948965580E+00;
    pub const PI: f64 = 3.1415926535897931160E+00;
    pub const PI_LO: f64 = 1.2246467991473531772E-16;
    pub const TINY: f64 = 1.0e-300;
    pub const PIO2_HI: f64 = 1.57079632679489655800e+00;
    pub const PIO2_LO: f64 = 6.12323399573676603587e-17;
    pub const PIO4_HI: f64 = 7.85398163397448278999e-01;
    pub const PS0: f64 = 1.66666666666666657415e-01;
    pub const PS1: f64 = -3.25565818622400915405e-01;
    pub const PS2: f64 = 2.01212532134862925881e-01;
    pub const PS3: f64 = -4.00555345006794114027e-02;
    pub const PS4: f64 = 7.91534994289814532176e-04;
    pub const PS5: f64 = 3.47933107596021167570e-05;
    pub const QS1: f64 = -2.40339491173441421878e+00;
    pub const QS2: f64 = 2.02094576023350569471e+00;
    pub const QS3: f64 = -6.88283971605453293030e-01;
    pub const QS4: f64 = 7.70381505559019352791e-02;
}
use fdlibm_constants::*;

fn hi(x: f64) -> i32 {
    (x.to_bits() >> 32) as i32
}

fn lo(x: f64) -> u32 {
    x.to_bits() as u32
}

/// fdlibm `atan(x)` (`s_atan.c`).
pub fn atan(x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fff_ffff;
    let mut x = x;
    let id: i32;
    if ix >= 0x4410_0000 {
        // |x| >= 2^66
        if ix > 0x7ff0_0000 || (ix == 0x7ff0_0000 && lo(x) != 0) {
            return x + x; // NaN
        }
        return if hx > 0 {
            ATAN_HI[3] + ATAN_LO[3]
        } else {
            -ATAN_HI[3] - ATAN_LO[3]
        };
    }
    if ix < 0x3fdc_0000 {
        // |x| < 0.4375
        if ix < 0x3e20_0000 {
            // |x| < 2^-29
            if 1.0e300 + x > 1.0 {
                return x;
            }
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3ff3_0000 {
            // |x| < 1.1875
            if ix < 0x3fe6_0000 {
                // 7/16 <= |x| < 11/16
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                // 11/16 <= |x| < 19/16
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x4003_8000 {
            // |x| < 2.4375
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            // 2.4375 <= |x| < 2^66
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * (A_T[0] + w * (A_T[2] + w * (A_T[4] + w * (A_T[6] + w * (A_T[8] + w * A_T[10])))));
    let s2 = w * (A_T[1] + w * (A_T[3] + w * (A_T[5] + w * (A_T[7] + w * A_T[9]))));
    if id < 0 {
        x - x * (s1 + s2)
    } else {
        let id = id as usize;
        let z = ATAN_HI[id] - ((x * (s1 + s2) - ATAN_LO[id]) - x);
        if hx < 0 { -z } else { z }
    }
}

/// `Math.atan2(y, x)` = fdlibm `__ieee754_atan2` (`e_atan2.c`).
pub fn atan2(y: f64, x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fff_ffff;
    let lx = lo(x);
    let hy = hi(y);
    let iy = hy & 0x7fff_ffff;
    let ly = lo(y);
    // x or y is NaN
    if (ix as u32 | ((lx | lx.wrapping_neg()) >> 31)) > 0x7ff0_0000
        || (iy as u32 | ((ly | ly.wrapping_neg()) >> 31)) > 0x7ff0_0000
    {
        return x + y;
    }
    if (hx.wrapping_sub(0x3ff0_0000) as u32 | lx) == 0 {
        return atan(y); // x = 1.0
    }
    let m = ((hy >> 31) & 1) | ((hx >> 30) & 2); // 2*sign(x)+sign(y)

    // when y = 0
    if (iy as u32 | ly) == 0 {
        return match m {
            0 | 1 => y,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    // when x = 0
    if (ix as u32 | lx) == 0 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    // when x is INF
    if ix == 0x7ff0_0000 {
        if iy == 0x7ff0_0000 {
            return match m {
                0 => PI_O_4 + TINY,
                1 => -PI_O_4 - TINY,
                2 => 3.0 * PI_O_4 + TINY,
                _ => -3.0 * PI_O_4 - TINY,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    // when y is INF
    if iy == 0x7ff0_0000 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    // compute y/x
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        PI_O_2 + 0.5 * PI_LO // |y/x| > 2**60
    } else if hx < 0 && k < -60 {
        0.0 // |y|/x < -2**60
    } else {
        atan((y / x).abs()) // safe to do y/x
    };
    match m {
        0 => z,
        1 => f64::from_bits(z.to_bits() ^ 0x8000_0000_0000_0000),
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

/// `Math.asin(x)` = fdlibm `__ieee754_asin` (`e_asin.c`).
pub fn asin(x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fff_ffff;
    let t: f64;
    if ix >= 0x3ff0_0000 {
        // |x| >= 1
        if (ix.wrapping_sub(0x3ff0_0000) as u32 | lo(x)) == 0 {
            return x * PIO2_HI + x * PIO2_LO; // asin(1) = +-pi/2
        }
        return f64::NAN; // fdlibm (x-x)/(x-x): asin(|x| > 1) is NaN
    } else if ix < 0x3fe0_0000 {
        // |x| < 0.5
        let t0 = if ix < 0x3e40_0000 {
            // |x| < 2**-27
            if 1.0e300 + x > 1.0 {
                return x;
            }
            0.0
        } else {
            x * x
        };
        let p = t0 * (PS0 + t0 * (PS1 + t0 * (PS2 + t0 * (PS3 + t0 * (PS4 + t0 * PS5)))));
        let q = 1.0 + t0 * (QS1 + t0 * (QS2 + t0 * (QS3 + t0 * QS4)));
        let w = p / q;
        return x + x * w;
    } else {
        // 1 > |x| >= 0.5
        let w = 1.0 - x.abs();
        let t1 = w * 0.5;
        let p = t1 * (PS0 + t1 * (PS1 + t1 * (PS2 + t1 * (PS3 + t1 * (PS4 + t1 * PS5)))));
        let q = 1.0 + t1 * (QS1 + t1 * (QS2 + t1 * (QS3 + t1 * QS4)));
        let s = t1.sqrt();
        if ix >= 0x3fef_3333 {
            // |x| > 0.975
            let w = p / q;
            t = PIO2_HI - (2.0 * (s + s * w) - PIO2_LO);
        } else {
            let w = f64::from_bits(s.to_bits() & 0xffff_ffff_0000_0000);
            let c = (t1 - w * w) / (s + w);
            let r = p / q;
            let p = 2.0 * s * r - (PIO2_LO - 2.0 * c);
            let q = PIO4_HI - 2.0 * w;
            t = PIO4_HI - (p - q);
        }
    }
    if hx > 0 { t } else { -t }
}

/// `Math.round(double)` — rounds half toward positive infinity (`-2.5` rounds
/// to `-2`, unlike Rust's `f64::round`), exactly (`0.49999999999999994` rounds
/// to `0`), saturating at the `long` range with `NaN` mapping to `0`.
pub fn round(a: f64) -> i64 {
    let floor = a.floor();
    // `a - floor(a)` is exact for every finite double.
    if a - floor >= 0.5 {
        (floor + 1.0) as i64
    } else {
        floor as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atan2_matches_java_for_the_quadrant_axes() {
        // Java: Math.atan2(1, 1) == Math.PI / 4, atan2(0, -1) == Math.PI.
        assert_eq!(atan2(1.0, 1.0), std::f64::consts::FRAC_PI_4);
        assert_eq!(atan2(0.0, -1.0), std::f64::consts::PI);
        assert_eq!(atan2(-0.0, -1.0), -std::f64::consts::PI);
        assert_eq!(atan2(1.0, 0.0), std::f64::consts::FRAC_PI_2);
        assert!(atan2(f64::NAN, 1.0).is_nan());
    }

    #[test]
    fn asin_matches_java_at_the_ends() {
        assert_eq!(asin(1.0), std::f64::consts::FRAC_PI_2);
        assert_eq!(asin(-1.0), -std::f64::consts::FRAC_PI_2);
        assert_eq!(asin(0.0).to_bits(), 0.0f64.to_bits());
        assert!(asin(1.0000001).is_nan());
    }

    #[test]
    fn round_is_java_math_round() {
        assert_eq!(round(2.5), 3);
        assert_eq!(round(-2.5), -2);
        assert_eq!(round(-2.6), -3);
        assert_eq!(round(0.49999999999999994), 0);
        assert_eq!(round(f64::NAN), 0);
        assert_eq!(round(f64::INFINITY), i64::MAX);
        assert_eq!(round(f64::NEG_INFINITY), i64::MIN);
    }
}
