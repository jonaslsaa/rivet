//! Moonrise's tick-thread-confined world RNG. The LCG and Gaussian mechanics
//! match LegacyRandomSource; the positional factory deliberately does not.
use super::{BitRandomSource, LegacyRandomSource, PositionalRandomFactory, RandomSource};
use super::{get_seed, java_string_hash};

pub struct ThreadUnsafeRandom(LegacyRandomSource);

impl ThreadUnsafeRandom {
    pub fn new(seed: i64) -> Self {
        Self(LegacyRandomSource::new(seed))
    }
}

impl BitRandomSource for ThreadUnsafeRandom {
    fn next(&mut self, bits: i32) -> i32 {
        self.0.next(bits)
    }
}

impl RandomSource for ThreadUnsafeRandom {
    type Positional = ThreadUnsafeRandomPositionalFactory;

    fn fork(&mut self) -> Self {
        Self::new(RandomSource::next_long(self))
    }

    fn fork_positional(&mut self) -> Self::Positional {
        ThreadUnsafeRandomPositionalFactory::new(RandomSource::next_long(self))
    }

    fn set_seed(&mut self, seed: i64) {
        self.0.set_seed(seed);
    }

    fn next_int(&mut self) -> i32 {
        BitRandomSource::next_int(self)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        BitRandomSource::next_int_bound(self, bound)
    }

    fn next_long(&mut self) -> i64 {
        BitRandomSource::next_long(self)
    }

    fn next_boolean(&mut self) -> bool {
        BitRandomSource::next_boolean(self)
    }

    fn next_float(&mut self) -> f32 {
        BitRandomSource::next_float(self)
    }

    fn next_double(&mut self) -> f64 {
        BitRandomSource::next_double(self)
    }

    fn next_gaussian(&mut self) -> f64 {
        self.0.next_gaussian()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ThreadUnsafeRandomPositionalFactory {
    seed: i64,
}

impl ThreadUnsafeRandomPositionalFactory {
    pub fn new(seed: i64) -> Self {
        Self { seed }
    }

    pub fn get_seed(&self) -> i64 {
        self.seed
    }
}

impl PositionalRandomFactory for ThreadUnsafeRandomPositionalFactory {
    type Output = ThreadUnsafeRandom;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Output {
        ThreadUnsafeRandom::new(get_seed(x, y, z) ^ self.seed)
    }

    fn from_hash_of(&self, name: &str) -> Self::Output {
        ThreadUnsafeRandom::new(i64::from(java_string_hash(name)) ^ self.seed)
    }

    fn from_seed(&self, seed: i64) -> Self::Output {
        // Unlike LegacyPositionalRandomFactory, Moonrise ignores the factory seed.
        ThreadUnsafeRandom::new(seed)
    }

    fn parity_config_string(&self, sb: &mut String) {
        use std::fmt::Write;
        write!(sb, "ThreadUnsafeRandomPositionalFactory{{{}}}", self.seed).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::{ThreadUnsafeRandom, ThreadUnsafeRandomPositionalFactory};
    use crate::random::{PositionalRandomFactory, RandomSource};
    use std::fmt::Write;

    fn sample(random: &mut ThreadUnsafeRandom, output: &mut String) {
        writeln!(
            output,
            "{} {} {} {} {} {} {} {}",
            random.next_int(),
            random.next_int_bound(1073741825),
            random.next_long(),
            random.next_boolean(),
            random.next_float().to_bits() as i32,
            random.next_double().to_bits() as i64,
            random.next_gaussian().to_bits() as i64,
            random.next_gaussian().to_bits() as i64
        )
        .unwrap();
    }

    #[test]
    fn thread_unsafe_random_matches_pinned_paper() {
        let mut output = String::new();
        for seed in [0, 1, -1, i64::MIN, i64::MAX, 123456789012345] {
            let mut random = ThreadUnsafeRandom::new(seed);
            sample(&mut random, &mut output);
            sample(&mut random.fork(), &mut output);
            let factory: ThreadUnsafeRandomPositionalFactory = random.fork_positional();
            write!(output, "{} ", factory.get_seed()).unwrap();
            factory.parity_config_string(&mut output);
            output.push('\n');
            sample(&mut factory.at(-30000000, -64, 29999999), &mut output);
            sample(&mut factory.from_hash_of("minecraft:bee🐝"), &mut output);
            sample(&mut factory.from_seed(42), &mut output);
            random.next_gaussian();
            random.set_seed(seed);
            sample(&mut random, &mut output);
            sample(&mut random, &mut output);
        }
        assert_eq!(output, include_str!("thread_unsafe_random_paper.txt"));
    }
}
