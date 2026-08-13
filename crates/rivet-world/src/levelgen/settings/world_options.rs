//! Port of `net.minecraft.world.level.levelgen.WorldOptions` (26.2) — the
//! `mc.world.level.levelgen.settings` unit.
//!
//! The seed/structures/bonus-chest world options record carried by
//! `WorldGenSettings` (and by `PrimaryLevelData` in level.dat). Java is a plain
//! immutable class (not a record) with a private 4-arg constructor; the codec
//! applies that constructor, so the port keeps the 3-arg `new` public and adds
//! the codec-shape `new_with_legacy_custom_options`.
//!
//! The `CODEC` is a `RecordCodecBuilder.mapCodec` over four fields, each
//! stamped `.stable()` and the whole group `.apply(i, i.stable(...))`:
//! `"seed"` (`Codec.LONG`), `"generate_structures"` /
//! `"bonus_chest"` (`ExtraCodecs.optionalAlwaysPresentFieldOf`, default `true`
//! / `false`), and `"legacy_custom_options"`
//! (`Codec.STRING.lenientOptionalFieldOf`, the old customized-world string).
//!
//! `ExtraCodecs.optionalAlwaysPresentFieldOf` is not ported as a helper; the
//! port composes it inline from `Codec.optionalField(name, codec, false)`
//! xmapped with `orElse(default)` / `Optional::of` (the exact Java body), so
//! the field is required on decode and *always* written on encode (unlike
//! `optionalFieldOf(name, default)`, which omits the default).

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::java_hash::string_hash;
use rivet_util::random::{RandomSource, random_source_create};
use std::sync::{Arc, LazyLock};

/// `net.minecraft.world.level.levelgen.WorldOptions`.
///
/// Java is a plain immutable class; the port derives value `PartialEq`/`Eq`
/// (all fields final; Java relies on reference equality but no current consumer
/// observes it — `WorldGenSettings.hashCode` combines the two fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldOptions {
    /// `seed`.
    seed: i64,
    /// `generateStructures`.
    generate_structures: bool,
    /// `generateBonusChest`.
    generate_bonus_chest: bool,
    /// `legacyCustomOptions` — the old customized-world options string.
    legacy_custom_options: Option<String>,
}

/// `WorldOptions.DEMO_OPTIONS` — `new WorldOptions("North Carolina".hashCode(),
/// true, true)` (Java's `String.hashCode`, widened `int` -> `long`).
pub static DEMO_OPTIONS: LazyLock<WorldOptions> =
    LazyLock::new(|| WorldOptions::new(string_hash("North Carolina") as i64, true, true));

impl WorldOptions {
    /// `WorldOptions(long, boolean, boolean)` — the 3-arg public constructor,
    /// delegating to the private 4-arg one with no legacy custom options.
    pub fn new(seed: i64, generate_structures: bool, generate_bonus_chest: bool) -> Self {
        WorldOptions::new_with_legacy_custom_options(
            seed,
            generate_structures,
            generate_bonus_chest,
            None,
        )
    }

    /// The private 4-arg constructor (the codec's `apply` function).
    pub fn new_with_legacy_custom_options(
        seed: i64,
        generate_structures: bool,
        generate_bonus_chest: bool,
        legacy_custom_options: Option<String>,
    ) -> Self {
        WorldOptions {
            seed,
            generate_structures,
            generate_bonus_chest,
            legacy_custom_options,
        }
    }

    /// `defaultWithRandomSeed()` — `new WorldOptions(randomSeed(), true, false)`.
    pub fn default_with_random_seed() -> Self {
        WorldOptions::new(Self::random_seed(), true, false)
    }

    /// `testWorldWithRandomSeed()` — `new WorldOptions(randomSeed(), false,
    /// false)`.
    pub fn test_world_with_random_seed() -> Self {
        WorldOptions::new(Self::random_seed(), false, false)
    }

    /// `seed()`.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// `generateStructures()`.
    pub fn generate_structures(&self) -> bool {
        self.generate_structures
    }

    /// `generateBonusChest()`.
    pub fn generate_bonus_chest(&self) -> bool {
        self.generate_bonus_chest
    }

    /// `isOldCustomizedWorld()` — `this.legacyCustomOptions.isPresent()`.
    pub fn is_old_customized_world(&self) -> bool {
        self.legacy_custom_options.is_some()
    }

    /// `withBonusChest(boolean)` — a copy with the bonus-chest flag replaced.
    pub fn with_bonus_chest(&self, generate_bonus_chest: bool) -> Self {
        WorldOptions::new_with_legacy_custom_options(
            self.seed,
            self.generate_structures,
            generate_bonus_chest,
            self.legacy_custom_options.clone(),
        )
    }

    /// `withStructures(boolean)` — a copy with the structures flag replaced.
    pub fn with_structures(&self, generate_structures: bool) -> Self {
        WorldOptions::new_with_legacy_custom_options(
            self.seed,
            generate_structures,
            self.generate_bonus_chest,
            self.legacy_custom_options.clone(),
        )
    }

    /// `withSeed(OptionalLong)` — a copy with the seed replaced; an empty
    /// `OptionalLong` falls back to `randomSeed()`.
    pub fn with_seed(&self, seed: Option<i64>) -> Self {
        WorldOptions::new_with_legacy_custom_options(
            seed.unwrap_or_else(Self::random_seed),
            self.generate_structures,
            self.generate_bonus_chest,
            self.legacy_custom_options.clone(),
        )
    }

    /// `parseSeed(String)` — trim; empty -> `OptionalLong.empty()`; parse as a
    /// `long`, falling back to `String.hashCode()` (Java `int` widened to
    /// `long`) on `NumberFormatException`.
    pub fn parse_seed(seed_string: &str) -> Option<i64> {
        let seed_string = seed_string.trim();
        if seed_string.is_empty() {
            return None;
        }
        match seed_string.parse::<i64>() {
            Ok(seed) => Some(seed),
            Err(_) => Some(string_hash(seed_string) as i64),
        }
    }

    /// `randomSeed()` — `RandomSource.create().nextLong()`.
    pub fn random_seed() -> i64 {
        let mut random = random_source_create();
        random.next_long()
    }
}

/// `WorldOptions.CODEC` — the ops-generic `world_options_map_codec::<Ops>()`
/// factory (a `MapCodec`, matching Java's `RecordCodecBuilder.mapCodec`).
///
/// Each field is stamped `.stable()`; the composed codec is `.stable()` too
/// (Java's `.apply(i, i.stable(WorldOptions::new))`). `"seed"` is a required
/// `LONG` field; `"generate_structures"`/`"bonus_chest"` are
/// `optionalAlwaysPresentFieldOf` (required on decode, always written on
/// encode); `"legacy_custom_options"` is a lenient optional string.
pub fn world_options_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<WorldOptions, Ops>>
{
    map_codec::stable(record_builder::map_codec(|instance| {
        let seed_field = map_codec::stable(codec::field_of(
            codec::long_codec::<Ops>(),
            "seed".to_string(),
        ));
        // `ExtraCodecs.optionalAlwaysPresentFieldOf(Codec.BOOL, ..., true)`
        // (see the module doc for the composition).
        let generate_structures_field = map_codec::stable(map_codec::xmap(
            codec::optional_field(
                "generate_structures".to_string(),
                codec::bool_codec::<Ops>(),
                false,
            ),
            Arc::new(|o: &Option<bool>| o.unwrap_or(true)),
            Arc::new(|v: &bool| Some(*v)),
        ));
        // `optionalAlwaysPresentFieldOf(Codec.BOOL, ..., false)`.
        let bonus_chest_field = map_codec::stable(map_codec::xmap(
            codec::optional_field("bonus_chest".to_string(), codec::bool_codec::<Ops>(), false),
            Arc::new(|o: &Option<bool>| o.unwrap_or(false)),
            Arc::new(|v: &bool| Some(*v)),
        ));
        // `Codec.STRING.lenientOptionalFieldOf("legacy_custom_options")`.
        let legacy_custom_options_field = map_codec::stable(codec::optional_field(
            "legacy_custom_options".to_string(),
            codec::string_codec::<Ops>(),
            true,
        ));
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|w: &WorldOptions| w.seed),
                seed_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|w: &WorldOptions| w.generate_structures),
                generate_structures_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|w: &WorldOptions| w.generate_bonus_chest),
                bonus_chest_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|w: &WorldOptions| w.legacy_custom_options.clone()),
                legacy_custom_options_field,
            ))
            .apply(
                instance,
                Arc::new(WorldOptions::new_with_legacy_custom_options),
            )
    }))
}

/// `WorldOptions.CODEC` lifted to a full `Codec` — `map_codec::codec_of`.
pub fn world_options_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<WorldOptions, Ops>> {
    map_codec::codec_of(world_options_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn demo_options_is_north_carolina_hash() {
        // `"North Carolina".hashCode()` widened int -> long.
        assert_eq!(DEMO_OPTIONS.seed, string_hash("North Carolina") as i64);
        assert!(DEMO_OPTIONS.generate_structures);
        assert!(DEMO_OPTIONS.generate_bonus_chest);
    }

    #[test]
    fn parse_seed_matches_java() {
        // Empty/whitespace -> `OptionalLong.empty()`.
        assert_eq!(WorldOptions::parse_seed(""), None);
        assert_eq!(WorldOptions::parse_seed("   "), None);
        // `Long.parseLong` succeeds.
        assert_eq!(WorldOptions::parse_seed("123"), Some(123));
        assert_eq!(WorldOptions::parse_seed("-42"), Some(-42));
        assert_eq!(WorldOptions::parse_seed(" 42 "), Some(42));
        // `NumberFormatException` -> `String.hashCode()` (int widened).
        assert_eq!(
            WorldOptions::parse_seed("North Carolina"),
            Some(string_hash("North Carolina") as i64)
        );
        assert_eq!(
            WorldOptions::parse_seed("abc"),
            Some(string_hash("abc") as i64)
        );
    }

    #[test]
    fn seed_is_required_and_always_present_fields_encode() {
        let ops = JsonOps::INSTANCE;
        let codec = world_options_codec::<JsonOps>();
        let options = WorldOptions::new(12345, true, false);
        let encoded = codec
            .encode_start(&ops, &options)
            .result()
            .expect("encode")
            .clone();
        // `optionalAlwaysPresentFieldOf` always writes both bool fields.
        assert_eq!(
            encoded,
            json!({"seed": 12345, "generate_structures": true, "bonus_chest": false})
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, options);
    }

    #[test]
    fn missing_always_present_fields_default() {
        let ops = JsonOps::INSTANCE;
        let codec = world_options_codec::<JsonOps>();
        // Only `seed` present: both bool fields default (present-on-decode).
        let decoded = codec
            .parse(&ops, &json!({"seed": 7}))
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, WorldOptions::new(7, true, false));
    }

    #[test]
    fn legacy_custom_options_roundtrip_and_old_flag() {
        let ops = JsonOps::INSTANCE;
        let codec = world_options_codec::<JsonOps>();
        let options = WorldOptions::new_with_legacy_custom_options(
            1,
            true,
            false,
            Some("old_customized".to_string()),
        );
        assert!(options.is_old_customized_world());
        let encoded = codec
            .encode_start(&ops, &options)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded["legacy_custom_options"], "old_customized");
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, options);
        // Lenient absent -> None.
        let plain = codec
            .parse(
                &ops,
                &json!({"seed": 1, "generate_structures": true, "bonus_chest": false}),
            )
            .result()
            .expect("decode")
            .clone();
        assert!(!plain.is_old_customized_world());
    }

    #[test]
    fn with_seed_and_flags_copy() {
        let base = WorldOptions::new(5, true, false);
        assert_eq!(base.with_seed(Some(9)).seed, 9);
        assert!(base.with_seed(None).seed != 5); // falls back to randomSeed()
        assert!(base.with_bonus_chest(true).generate_bonus_chest);
        assert!(!base.with_structures(false).generate_structures);
        // The source is unchanged (immutable copies).
        assert_eq!(base, WorldOptions::new(5, true, false));
    }
}
