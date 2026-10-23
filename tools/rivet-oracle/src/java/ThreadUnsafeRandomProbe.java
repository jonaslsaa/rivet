import ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom;
import net.minecraft.util.RandomSource;

public final class ThreadUnsafeRandomProbe {
    private static void sample(RandomSource random) {
        System.out.println(random.nextInt() + " " + random.nextInt(1073741825)
            + " " + random.nextLong() + " " + random.nextBoolean()
            + " " + Float.floatToRawIntBits(random.nextFloat())
            + " " + Double.doubleToRawLongBits(random.nextDouble())
            + " " + Double.doubleToRawLongBits(random.nextGaussian())
            + " " + Double.doubleToRawLongBits(random.nextGaussian()));
    }

    public static void main(String[] args) {
        for (long seed : new long[] {0, 1, -1, Long.MIN_VALUE, Long.MAX_VALUE, 123456789012345L}) {
            ThreadUnsafeRandom random = new ThreadUnsafeRandom(seed);
            sample(random);
            sample(random.fork());
            var factory = (ThreadUnsafeRandom.ThreadUnsafeRandomPositionalFactory) random.forkPositional();
            StringBuilder description = new StringBuilder();
            factory.parityConfigString(description);
            System.out.println(factory.getSeed() + " " + description);
            sample(factory.at(-30000000, -64, 29999999));
            sample(factory.fromHashOf("minecraft:bee\uD83D\uDC1D"));
            sample(factory.fromSeed(42));
            random.nextGaussian();
            random.setSeed(seed);
            sample(random);
            sample(random);
        }
    }
}
