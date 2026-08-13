//! `net.minecraft.util.LinearCongruentialGenerator` — the pure LCG step used
//! by `BiomeManager.getFiddledDistance` (the `mc.world.level.biome` unit's
//! fiddled-distance corner interpolation).
//!
//! Java (12-line class):
//!
//! ```java
//! public class LinearCongruentialGenerator {
//!     private static final long MULTIPLIER = 6364136223846793005L;
//!     private static final long INCREMENT = 1442695040888963407L;
//!     public static long next(long rval, final long c) {
//!         rval *= rval * MULTIPLIER + INCREMENT;
//!         return rval + c;
//!     }
//! }
//! ```
//!
//! Both arithmetic steps wrap (`*`/`+` on Java `long`), so the port uses
//! `wrapping_mul`/`wrapping_add`.

/// `LinearCongruentialGenerator.next(long rval, long c)` — one LCG step:
/// `rval = rval * (rval * MULTIPLIER + INCREMENT); rval + c`, all wrapping.
pub fn next(mut rval: i64, c: i64) -> i64 {
    rval = rval.wrapping_mul(rval.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT));
    rval.wrapping_add(c)
}

/// `LinearCongruentialGenerator.MULTIPLIER`.
const MULTIPLIER: i64 = 6364136223846793005;

/// `LinearCongruentialGenerator.INCREMENT`.
const INCREMENT: i64 = 1442695040888963407;

#[cfg(test)]
mod tests {
    use super::next;

    /// Golden values produced by running the reference implementation on
    /// OpenJDK 25 (the Java runtime in this environment), not invented.
    #[test]
    fn next_golden() {
        assert_eq!(next(0, 0), 0);
        assert_eq!(next(1, 0), 7806831264735756412);
        assert_eq!(next(1, 1), 7806831264735756413);
        assert_eq!(next(42, 0), -2477882527166265078);
        assert_eq!(next(0, 7), 7);
        // Wrapping: the first step itself wraps for a large rval.
        assert_eq!(next(i64::MAX, 0), -4301930853896946210);
        assert_eq!(next(-1, 0), 4921441182957829598);
    }

    #[test]
    fn next_is_deterministic_and_c_shifts_result() {
        // The same (rval, c) always yields the same result.
        assert_eq!(next(12345, 67890), next(12345, 67890));
        // A different c shifts the result by exactly c (the final add).
        assert_eq!(next(12345, 1), next(12345, 0).wrapping_add(1));
        // Pinned: the (12345, 67890) goldens from the Java run.
        assert_eq!(next(12345, 67890), -4650592213636444698);
        assert_eq!(next(12345, 1), -4650592213636512587);
    }
}
