//! `net.minecraft.world.level.levelgen.PositionalRandomFactory` — the Java
//! **default** overloads taking `BlockPos` / `Identifier` (issue #208).
//!
//! The base `PositionalRandomFactory` trait (the `at(int,int,int)` /
//! `fromHashOf(String)` / `fromSeed(long)` / `parityConfigString` surface) lives
//! in `rivet-util::random` — it is registry-free. The two Java default
//! interface methods that sit ON TOP of it need `BlockPos` and `Identifier`,
//! which live in `rivet-registry` (a dependency `rivet-util` cannot take without
//! a Cargo cycle). They are ported here instead, in the `mc.world.level.levelgen`
//! crate (`rivet-world`, per MANIFEST.tsv):
//!
//! ```java
//! default RandomSource at(final BlockPos pos) { return this.at(pos.getX(), pos.getY(), pos.getZ()); }
//! default RandomSource fromHashOf(final Identifier name) { return this.fromHashOf(name.toString()); }
//! ```
//!
//! `PositionalRandomFactoryOverloads` is the blanket-implemented extension of
//! the base trait; `at_pos` / `from_hash_of_identifier` return the base trait's
//! associated `Output` type (`Self::Output`, the `RandomSource` each factory
//! yields) and delegate to the exact base-trait calls the Java defaults
//! delegate to, so the seed derived is identical (verified by the `SeqProbe`
//! goldens in `tools/rivet-oracle/fixtures/seq/seq-random.json`).

use rivet_registry::Identifier;
use rivet_registry::core::BlockPos;
use rivet_util::random::PositionalRandomFactory;

/// The `PositionalRandomFactory` BlockPos/Identifier default overloads (issue
/// #208) — Java default methods that delegate to the registry-free forms.
// `at_pos` / `from_hash_of_identifier` mirror Java default methods; the
// `from_*`-taking-`&self` naming trips clippy's `wrong_self_convention` the
// same way the base trait's `from_hash_of` does (a false positive here —
// renaming would break the Java-method-name fidelity).
#[allow(clippy::wrong_self_convention)]
pub trait PositionalRandomFactoryOverloads: PositionalRandomFactory {
    /// `PositionalRandomFactory.at(BlockPos)` — `this.at(pos.getX(), pos.getY(), pos.getZ())`.
    fn at_pos(&self, pos: &BlockPos) -> Self::Output {
        self.at(pos.get_x(), pos.get_y(), pos.get_z())
    }

    /// `PositionalRandomFactory.fromHashOf(Identifier)` — `this.fromHashOf(name.toString())`.
    fn from_hash_of_identifier(&self, name: &Identifier) -> Self::Output {
        self.from_hash_of(&name.to_string())
    }
}

impl<T: PositionalRandomFactory> PositionalRandomFactoryOverloads for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_util::random::{LegacyPositionalRandomFactory, XoroshiroPositionalRandomFactory};
    use serde_json::Value;
    use std::path::PathBuf;

    /// The Paper-captured golden fixture (`SeqProbe`, issue #208). Both the
    /// `legacy` and `xoroshiro` factories are seeded the same way the Rust
    /// constructors below seed them, and each sample is the raw
    /// `nextInt()` x3 / `nextLong()` x2 output of the source yielded by the
    /// Java default overload — exactly what `at_pos` / `from_hash_of_identifier`
    /// must reproduce. The `overworld` row (no colon) locks
    /// `Identifier.parse`'s default-namespace normalization followed by Java
    /// `toString()`: it must reproduce the `minecraft:overworld` row exactly.
    fn fixture() -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/seq/seq-random.json");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!("missing Paper SeqProbe fixture at {}: {e}", path.display())
        });
        serde_json::from_slice(&bytes).expect("valid seq-random.json")
    }

    fn draw_ints_and_longs<R: rivet_util::random::RandomSource>(mut r: R) -> (Vec<i64>, Vec<i64>) {
        (
            vec![
                r.next_int() as i64,
                r.next_int() as i64,
                r.next_int() as i64,
            ],
            vec![r.next_long(), r.next_long()],
        )
    }

    fn json_i64s(entry: &Value) -> Vec<i64> {
        entry
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect()
    }

    fn check_factory(name: &str, factory: impl PositionalRandomFactory, cases: &Value) {
        let at = cases.get("at").expect("at cases");
        for entry in at.as_array().unwrap() {
            let pos = entry.get("pos").unwrap().as_array().unwrap();
            let (x, y, z) = (
                pos[0].as_i64().unwrap() as i32,
                pos[1].as_i64().unwrap() as i32,
                pos[2].as_i64().unwrap() as i32,
            );
            let (ints, longs) = draw_ints_and_longs(factory.at_pos(&BlockPos::new(x, y, z)));
            assert_eq!(
                ints,
                json_i64s(&entry["ints"]),
                "{name} at({x},{y},{z}) ints"
            );
            assert_eq!(
                longs,
                json_i64s(&entry["longs"]),
                "{name} at({x},{y},{z}) longs"
            );
        }

        let from_hash_of = cases.get("fromHashOf").expect("fromHashOf cases");
        for entry in from_hash_of.as_array().unwrap() {
            let id = entry.get("id").unwrap().as_str().unwrap();
            let identifier = Identifier::parse(id);
            let (ints, longs) = draw_ints_and_longs(factory.from_hash_of_identifier(&identifier));
            assert_eq!(
                ints,
                json_i64s(&entry["ints"]),
                "{name} fromHashOf({id}) ints"
            );
            assert_eq!(
                longs,
                json_i64s(&entry["longs"]),
                "{name} fromHashOf({id}) longs"
            );
        }
    }

    #[test]
    fn positional_factory_overloads_match_paper_seqprobe() {
        let factories = &fixture()["factories"];
        check_factory(
            "legacy",
            LegacyPositionalRandomFactory::new(99),
            &factories["legacy"],
        );
        check_factory(
            "xoroshiro",
            XoroshiroPositionalRandomFactory::new(99, 1234),
            &factories["xoroshiro"],
        );
    }
}
