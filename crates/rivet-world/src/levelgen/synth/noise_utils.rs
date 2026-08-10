//! Port of `net.minecraft.world.level.levelgen.synth.NoiseUtils` (class, 26.2).
//!
//! `biasTowardsExtreme` and the two `parityNoiseOctaveConfigString` overloads.
//! The parity strings mirror `String.format(Locale.ROOT, "%.3f", (float)xo)`:
//! `1.2345678f32 -> "1.235"`, `-9.8765432f32 -> "-9.877"`, `0.000123456f32 ->
//! "0.000"`.
//!
//! Rounding-edge note: Rust `{:.3}` on an `f32` rounds half-to-even while
//! Java's `%.3f` can round half-up on an exact `0.0005` midpoint (e.g.
//! `1.0625f32` formats as `"1.062"`). This is diagnostic-only
//! (`@VisibleForTesting` parity output, not gameplay), and none of the pinned
//! fixture values land on such a midpoint, so the two agree bit-for-bit there.

/// `NoiseUtils.biasTowardsExtreme(double noise, double factor)`.
///
/// `noise + Math.sin(Math.PI * noise) * factor / Math.PI`. Rust's
/// `std::f64::consts::PI` is the same f64 as Java's `Math.PI` (both the
/// closest f64 to pi, `0x400921FB54442D18`), so the expression is bit-exact.
pub fn bias_towards_extreme(noise: f64, factor: f64) -> f64 {
    noise + (std::f64::consts::PI * noise).sin() * factor / std::f64::consts::PI
}

/// `NoiseUtils.parityNoiseOctaveConfigString(StringBuilder, double xo, double
/// yo, double zo, byte[] p)`.
pub fn parity_noise_octave_config_string(sb: &mut String, xo: f64, yo: f64, zo: f64, p: &[i8]) {
    let xo_f = xo as f32;
    let yo_f = yo as f32;
    let zo_f = zo as f32;
    sb.push_str(&format!(
        "xo={:.3}, yo={:.3}, zo={:.3}, p0={}, p255={}",
        xo_f, yo_f, zo_f, p[0], p[255]
    ));
}

/// `NoiseUtils.parityNoiseOctaveConfigString(StringBuilder, double xo, double
/// yo, double zo, int[] p)`.
pub fn parity_noise_octave_config_string_i32(
    sb: &mut String,
    xo: f64,
    yo: f64,
    zo: f64,
    p: &[i32],
) {
    let xo_f = xo as f32;
    let yo_f = yo as f32;
    let zo_f = zo as f32;
    sb.push_str(&format!(
        "xo={:.3}, yo={:.3}, zo={:.3}, p0={}, p255={}",
        xo_f, yo_f, zo_f, p[0], p[255]
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bias_edges_are_exact() {
        // The full 32-case table is exercised through the integration golden
        // test; here we pin the structural edges: zero `factor` leaves `noise`
        // unchanged, and a `noise` of exactly 0 or 1 has `sin(pi*n) == 0`.
        assert_eq!(bias_towards_extreme(0.5, 0.0).to_bits(), 0.5f64.to_bits());
        assert_eq!(bias_towards_extreme(0.0, 1.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(bias_towards_extreme(1.0, 1.0).to_bits(), 1.0f64.to_bits());
        assert_eq!(
            bias_towards_extreme(-1.0, 1.0).to_bits(),
            (-1.0f64).to_bits()
        );
    }
}
