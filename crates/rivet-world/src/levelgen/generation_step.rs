//! `net.minecraft.world.level.levelgen.GenerationStep` — the worldgen step
//! enum.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/levelgen/GenerationStep.java`. The outer `GenerationStep` class
//! is a pure namespace holder for its one nested `Decoration` enum (it has no
//! members of its own), so the port mirrors it as a unit struct plus the
//! sibling `Decoration` enum.
//!
//! PROVENANCE: `GenerationStep.java` is listed in the pending
//! `mc.world.level.levelgen.settings` manifest unit's `java_paths` (the #179
//! settings wave); it is proactively ported here because the feature-shell wave
//! (#306) needs `Decoration` to type-check `BiomeGenerationSettings`-style
//! feature-list keys. A later settings wave must not re-port it.
//!
//! `Decoration` is an 11-value `StringRepresentable` enum whose `CODEC` is
//! `StringRepresentable.fromEnum(GenerationStep.Decoration::values)`. The
//! `mc.util` port of `StringRepresentable` lives in
//! `rivet-util::string_representable`; its `from_enum` factory needs the
//! `EnumOrdinal` helper (Java's `Enum.ordinal()`, for which Rust has no
//! intrinsic), so `Decoration` implements it in declaration order. The
//! `CODEC` constant itself is ops-generic in the port — DFU `Codec<T>` is
//! `Codec<E, Ops>` here — so the static Java constant is exposed as the
//! `decoration_codec::<Ops>()` factory, the same shape `Rotations.CODEC` takes
//! in `rivet-registry::core::rotations`.
//!
//! Java consumers (all unported settings/worldgen units): `BiomeGenerationSettings`
//! keys feature lists by `Decoration`; `FlatLevelGeneratorSettings` compares
//! `Decoration.ordinal()`; `Structure` decodes `Decoration.CODEC`.

use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::string_representable::{self, EnumCodec, EnumOrdinal, StringRepresentable};

/// `GenerationStep` — the outer class; a namespace holder for `Decoration`.
/// Java never instantiates it, so this is a unit struct.
pub struct GenerationStep;

/// `GenerationStep.Decoration` — the eleven chunk-generation phases, in Java's
/// ordinal (declaration) order. The serialized name is the vanilla
/// worldgen key (`raw_generation`..`top_layer_modification`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Decoration {
    /// `RAW_GENERATION("raw_generation")`.
    RawGeneration,
    /// `LAKES("lakes")`.
    Lakes,
    /// `LOCAL_MODIFICATIONS("local_modifications")`.
    LocalModifications,
    /// `UNDERGROUND_STRUCTURES("underground_structures")`.
    UndergroundStructures,
    /// `SURFACE_STRUCTURES("surface_structures")`.
    SurfaceStructures,
    /// `STRONGHOLDS("strongholds")`.
    Strongholds,
    /// `UNDERGROUND_ORES("underground_ores")`.
    UndergroundOres,
    /// `UNDERGROUND_DECORATION("underground_decoration")`.
    UndergroundDecoration,
    /// `FLUID_SPRINGS("fluid_springs")`.
    FluidSprings,
    /// `VEGETAL_DECORATION("vegetal_decoration")`.
    VegetalDecoration,
    /// `TOP_LAYER_MODIFICATION("top_layer_modification")`.
    TopLayerModification,
}

impl Decoration {
    /// `Decoration.values()` — the constants in declaration order.
    pub const VALUES: [Decoration; 11] = [
        Decoration::RawGeneration,
        Decoration::Lakes,
        Decoration::LocalModifications,
        Decoration::UndergroundStructures,
        Decoration::SurfaceStructures,
        Decoration::Strongholds,
        Decoration::UndergroundOres,
        Decoration::UndergroundDecoration,
        Decoration::FluidSprings,
        Decoration::VegetalDecoration,
        Decoration::TopLayerModification,
    ];

    /// `Decoration.getName()`.
    pub fn get_name(&self) -> &'static str {
        self.get_serialized_name()
    }

    /// `Decoration.getSerializedName()`.
    pub fn get_serialized_name(&self) -> &'static str {
        match self {
            Decoration::RawGeneration => "raw_generation",
            Decoration::Lakes => "lakes",
            Decoration::LocalModifications => "local_modifications",
            Decoration::UndergroundStructures => "underground_structures",
            Decoration::SurfaceStructures => "surface_structures",
            Decoration::Strongholds => "strongholds",
            Decoration::UndergroundOres => "underground_ores",
            Decoration::UndergroundDecoration => "underground_decoration",
            Decoration::FluidSprings => "fluid_springs",
            Decoration::VegetalDecoration => "vegetal_decoration",
            Decoration::TopLayerModification => "top_layer_modification",
        }
    }
}

impl StringRepresentable for Decoration {
    fn get_serialized_name(&self) -> &str {
        Decoration::get_serialized_name(self)
    }
}

impl EnumOrdinal for Decoration {
    fn ordinal(&self) -> usize {
        match self {
            Decoration::RawGeneration => 0,
            Decoration::Lakes => 1,
            Decoration::LocalModifications => 2,
            Decoration::UndergroundStructures => 3,
            Decoration::SurfaceStructures => 4,
            Decoration::Strongholds => 5,
            Decoration::UndergroundOres => 6,
            Decoration::UndergroundDecoration => 7,
            Decoration::FluidSprings => 8,
            Decoration::VegetalDecoration => 9,
            Decoration::TopLayerModification => 10,
        }
    }
}

impl std::fmt::Display for Decoration {
    /// `Enum.toString()` — the constant name (`RAW_GENERATION`, ...), not the
    /// serialized key. Only observable through the (unreachable for a real
    /// enum) `id_resolver_codec` encode error `"Element with unknown id: " + e`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Decoration::RawGeneration => "RAW_GENERATION",
            Decoration::Lakes => "LAKES",
            Decoration::LocalModifications => "LOCAL_MODIFICATIONS",
            Decoration::UndergroundStructures => "UNDERGROUND_STRUCTURES",
            Decoration::SurfaceStructures => "SURFACE_STRUCTURES",
            Decoration::Strongholds => "STRONGHOLDS",
            Decoration::UndergroundOres => "UNDERGROUND_ORES",
            Decoration::UndergroundDecoration => "UNDERGROUND_DECORATION",
            Decoration::FluidSprings => "FLUID_SPRINGS",
            Decoration::VegetalDecoration => "VEGETAL_DECORATION",
            Decoration::TopLayerModification => "TOP_LAYER_MODIFICATION",
        })
    }
}

/// `GenerationStep.Decoration.CODEC` —
/// `StringRepresentable.fromEnum(GenerationStep.Decoration::values)`. The
/// port's DFU codecs are ops-generic, so the static Java constant becomes the
/// `decoration_codec::<Ops>()` factory (same shape as `Rotations.CODEC`).
pub fn decoration_codec<Ops: DynamicOps + 'static>() -> EnumCodec<Decoration, Ops> {
    string_representable::from_enum::<Decoration, Ops>(&Decoration::VALUES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::{Decoder, Encoder};

    #[test]
    fn names_and_ordinals() {
        // Serialized name and ordinal per the Java enum constants; `getName`
        // and `getSerializedName` both return the `name` field.
        let cases = [
            (Decoration::RawGeneration, 0, "raw_generation"),
            (Decoration::Lakes, 1, "lakes"),
            (Decoration::LocalModifications, 2, "local_modifications"),
            (
                Decoration::UndergroundStructures,
                3,
                "underground_structures",
            ),
            (Decoration::SurfaceStructures, 4, "surface_structures"),
            (Decoration::Strongholds, 5, "strongholds"),
            (Decoration::UndergroundOres, 6, "underground_ores"),
            (
                Decoration::UndergroundDecoration,
                7,
                "underground_decoration",
            ),
            (Decoration::FluidSprings, 8, "fluid_springs"),
            (Decoration::VegetalDecoration, 9, "vegetal_decoration"),
            (
                Decoration::TopLayerModification,
                10,
                "top_layer_modification",
            ),
        ];
        for (decoration, ordinal, name) in cases {
            assert_eq!(decoration.ordinal(), ordinal);
            assert_eq!(decoration.get_name(), name);
            assert_eq!(decoration.get_serialized_name(), name);
        }
    }

    #[test]
    fn display_is_the_java_constant_name() {
        // `Enum.toString()` returns the constant name, not the serialized key.
        assert_eq!(Decoration::RawGeneration.to_string(), "RAW_GENERATION");
        assert_eq!(
            Decoration::TopLayerModification.to_string(),
            "TOP_LAYER_MODIFICATION"
        );
    }

    #[test]
    fn values_is_declaration_order() {
        let expected = [
            Decoration::RawGeneration,
            Decoration::Lakes,
            Decoration::LocalModifications,
            Decoration::UndergroundStructures,
            Decoration::SurfaceStructures,
            Decoration::Strongholds,
            Decoration::UndergroundOres,
            Decoration::UndergroundDecoration,
            Decoration::FluidSprings,
            Decoration::VegetalDecoration,
            Decoration::TopLayerModification,
        ];
        assert_eq!(Decoration::VALUES, expected);
    }

    #[test]
    fn codec_roundtrips_all_values() {
        let ops = JsonOps::INSTANCE;
        let codec = decoration_codec::<JsonOps>();
        for value in Decoration::VALUES {
            // Encode to the serialized-name string.
            let encoded = codec
                .encode_start(&ops, &value)
                .get_or_throw("encode")
                .clone();
            assert_eq!(
                encoded,
                ops.create_string(value.get_serialized_name().to_string())
            );
            // Decode back (the orCompressed string branch).
            let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
            assert_eq!(decoded.0, value);
        }
    }

    #[test]
    fn codec_integer_ordinal_branch_only_under_compressed_ops() {
        // `orCompressed(stringResolver, idResolver)` routes on
        // `ops.compressMaps()` — the int-ordinal branch fires only for
        // compressed ops. Under `INSTANCE` (non-compressed) the id input goes
        // to the string branch and fails; under `COMPRESSED` it decodes the
        // ordinal.
        let normal = JsonOps::INSTANCE;
        let compressed = JsonOps::COMPRESSED;
        let codec = decoration_codec::<JsonOps>();
        assert!(
            codec
                .decode(&normal, &normal.create_int(0))
                .result()
                .is_none()
        );
        // The id branch decodes the ordinal.
        assert_eq!(
            codec
                .decode(&compressed, &compressed.create_int(0))
                .get_or_throw("decode")
                .0,
            Decoration::RawGeneration
        );
        assert_eq!(
            codec
                .decode(&compressed, &compressed.create_int(10))
                .get_or_throw("decode")
                .0,
            Decoration::TopLayerModification
        );
        // Encoding under compressed ops emits the ordinal int.
        assert_eq!(
            codec
                .encode_start(&compressed, &Decoration::Lakes)
                .get_or_throw("encode")
                .clone(),
            compressed.create_int(1)
        );
        // Out-of-range ids fail the id branch.
        assert!(
            codec
                .decode(&compressed, &compressed.create_int(11))
                .result()
                .is_none()
        );
        assert!(
            codec
                .decode(&compressed, &compressed.create_int(-1))
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_rejects_unknown_name() {
        let ops = JsonOps::INSTANCE;
        let codec = decoration_codec::<JsonOps>();
        let input = ops.create_string("not_a_step".to_string());
        assert!(codec.decode(&ops, &input).result().is_none());
    }

    #[test]
    fn codec_by_name() {
        let codec = decoration_codec::<JsonOps>();
        assert_eq!(codec.by_name("lakes"), Some(Decoration::Lakes));
        assert_eq!(
            codec.by_name("top_layer_modification"),
            Some(Decoration::TopLayerModification)
        );
        assert_eq!(codec.by_name("nope"), None);
        // `byName(name, _default)` — `Objects.requireNonNullElse`.
        assert_eq!(
            codec.by_name_or("nope", Decoration::Strongholds),
            Decoration::Strongholds
        );
    }
}
