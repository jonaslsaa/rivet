//! Port of `net.minecraft.world.level.biome.CheckerboardColumnBiomeSource`
//! (26.2) — the `mc.world.level.biome.source` unit.
//!
//! The checkerboard source: a `HolderSet<Biome>` and a `bitShift = size + 2`
//! that indexes the set by `floorMod((quartX >> bitShift) + (quartZ >>
//! bitShift), size)`:
//!
//! ```text
//! CODEC = RecordCodecBuilder.mapCodec(i -> i.group(
//!     Biome.LIST_CODEC.fieldOf("biomes"),
//!     Codec.intRange(0, 62).optionalFieldOf("scale", 2)
//! ).apply(i, CheckerboardColumnBiomeSource::new))
//! ```
//!
//! over the id-handle set codec (`biome_id_codec::biome_id_list_field_codec`,
//! the `Biome.LIST_CODEC` analogue). `Math.floorMod` is `rem_euclid`.
//!
//! The `>> this.bitShift` shift count is masked to the low 5 bits like Java's
//! JLS 15.19 semantics (`bitShift = size + 2` is unbounded — size is
//! `Codec.intRange(0, 62)`, so up to 64 — and Rust's runtime `>>` would panic in
//! debug builds for counts >= 32; see [`CheckerboardColumnBiomeSource::get_noise_biome`]).

use crate::biome::biome_id_codec::biome_id_list_field_codec;
use crate::biome::biome_resolver::BiomeResolver;
use crate::biome::biome_source::BiomeSource;
use crate::biome::biome_source_type::{BiomeSourceTypeId, BiomeSourceTypes};
use crate::biome::climate::Sampler;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::{Arc, OnceLock};

/// `CheckerboardColumnBiomeSource` — the checkerboard-over-quarts source.
#[derive(Debug)]
pub struct CheckerboardColumnBiomeSource {
    /// `this.allowedBiomes` — the `HolderSet<Biome>` the checkerboard indexes.
    pub allowed_biomes: HolderSet<BiomeId>,
    /// `this.bitShift` — `size + 2`.
    pub bit_shift: i32,
    /// `this.size` — the `"scale"` field (`Codec.intRange(0, 62)` default 2).
    pub size: i32,
    /// The `possibleBiomes` memo — Java's `Suppliers.memoize` (computed once on
    /// first read). Not part of equality (the derived cache value).
    possible_biomes: OnceLock<Vec<Holder<BiomeId>>>,
}

impl Clone for CheckerboardColumnBiomeSource {
    fn clone(&self) -> Self {
        CheckerboardColumnBiomeSource {
            allowed_biomes: self.allowed_biomes.clone(),
            bit_shift: self.bit_shift,
            size: self.size,
            possible_biomes: OnceLock::new(),
        }
    }
}

impl PartialEq for CheckerboardColumnBiomeSource {
    fn eq(&self, other: &Self) -> bool {
        self.allowed_biomes == other.allowed_biomes
            && self.bit_shift == other.bit_shift
            && self.size == other.size
    }
}

impl CheckerboardColumnBiomeSource {
    /// `new CheckerboardColumnBiomeSource(HolderSet<Biome>, int size)`.
    pub fn new(allowed_biomes: HolderSet<BiomeId>, size: i32) -> Self {
        CheckerboardColumnBiomeSource {
            allowed_biomes,
            bit_shift: size.wrapping_add(2),
            size,
            possible_biomes: OnceLock::new(),
        }
    }

    /// `CheckerboardColumnBiomeSource.CODEC`.
    pub fn map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn MapCodec<CheckerboardColumnBiomeSource, Ops>> {
        record_builder::map_codec(|instance| {
            instance
                .group(record_builder::RecordCodecBuilder::of(
                    Arc::new(|s: &CheckerboardColumnBiomeSource| s.allowed_biomes.clone()),
                    biome_id_list_field_codec("biomes"),
                ))
                .and(record_builder::RecordCodecBuilder::of(
                    Arc::new(|s: &CheckerboardColumnBiomeSource| s.size),
                    codec::optional_field_of("scale", codec::int_range::<Ops>(0, 62), 2),
                ))
                .apply(instance, Arc::new(CheckerboardColumnBiomeSource::new))
        })
    }
}

impl BiomeSource for CheckerboardColumnBiomeSource {
    fn type_id(&self) -> BiomeSourceTypeId {
        BiomeSourceTypes::CHECKERBOARD
    }

    fn collect_possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        self.allowed_biomes.iter().cloned().collect()
    }

    fn possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        self.possible_biomes
            .get_or_init(|| {
                crate::biome::biome_source::dedupe_possible_biomes(self.collect_possible_biomes())
            })
            .clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The source is its own resolver (Java `BiomeSource implements
/// BiomeResolver`): the checkerboard quart resolution.
impl BiomeResolver for CheckerboardColumnBiomeSource {
    fn get_noise_biome(
        &self,
        quart_x: i32,
        _quart_y: i32,
        quart_z: i32,
        _sampler: &Sampler,
    ) -> Holder<BiomeId> {
        // `this.allowedBiomes.get(Math.floorMod((quartX >> this.bitShift) +
        // (quartZ >> this.bitShift), this.allowedBiomes.size()))`.
        //
        // Java `bitShift = size + 2` is unbounded (size is `Codec.intRange(0,
        // 62)` → bitShift up to 64). JLS 15.19 masks an int shift count to the
        // low 5 bits, so `>> 64` is `>> 0` and `>> 63` is `>> 31` — never a
        // panic. Rust's `>>` on i32 with a runtime count >= 32 panics in debug
        // builds, so mask the count exactly like Java before shifting.
        let bit_shift_masked = (self.bit_shift & 31) as u32;
        let index = (quart_x >> bit_shift_masked)
            .wrapping_add(quart_z >> bit_shift_masked)
            .rem_euclid(self.allowed_biomes.size() as i32);
        self.allowed_biomes.get(index as usize).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::biome_source::biome_source_codec;
    use rivet_registry::RegistryAccess;
    use rivet_registry::holder::RegistryId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::{Identifier, RegistrationInfo, RegistryBuilder, ResourceKey};
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::{Value, json};
    use std::sync::Arc;

    /// The registry-backed ops the `BiomeSource` codecs require (the
    /// `"biomes"` list resolves the biome registry through the ops).
    type TestOps = RegistryOps<Value, JsonOps>;

    /// A `RegistryOps` whose access carries a biome registry with `plains`,
    /// `river` and `sunflower_plains` registered at element ids 0..2 — the
    /// `"biomes"` list decode resolves each identifier to a `Reference`.
    fn biome_ops() -> (TestOps, RegistryId) {
        let key = rivet_registry::registries::BIOME.clone();
        let mut builder = RegistryBuilder::<BiomeId>::new(&key);
        for (i, name) in ["plains", "river", "sunflower_plains"].iter().enumerate() {
            builder.register(
                &ResourceKey::create(&key, Identifier::with_default_namespace(name)),
                Arc::new(BiomeId::from_id(40 + i as u16)),
                RegistrationInfo::BUILT_IN,
            );
        }
        let registry = builder.freeze();
        let owner = registry.registry_id();
        let access = RegistryAccess::from_single_registry(key, registry);
        (
            RegistryOps::create_from_access(&JsonOps::INSTANCE, access),
            owner,
        )
    }

    fn set(items: &[u16]) -> HolderSet<BiomeId> {
        HolderSet::direct(
            items
                .iter()
                .map(|id| Holder::direct(BiomeId::from_id(*id)))
                .collect(),
        )
    }

    #[test]
    fn codec_round_trip_with_default_scale() {
        let codec = map_codec_of::<TestOps>();
        let (ops, owner) = biome_ops();
        // Decode produces `Reference` holders, so round-trip a set of
        // references. `optionalFieldOf` omits `"scale"` at its default, and the
        // two-element list keeps its list form.
        let src = CheckerboardColumnBiomeSource::new(
            HolderSet::direct(vec![
                Holder::reference(owner, 0),
                Holder::reference(owner, 1),
            ]),
            2,
        );
        let encoded = codec
            .encode_start(&ops, &src)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"biomes": ["minecraft:plains", "minecraft:river"]})
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, src);
    }

    #[test]
    fn scale_defaults_to_two_when_absent() {
        let codec = map_codec_of::<TestOps>();
        let (ops, owner) = biome_ops();
        let decoded = codec
            .parse(&ops, &json!({"biomes": ["minecraft:plains"]}))
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.size, 2);
        assert_eq!(decoded.bit_shift, 4);
        assert_eq!(
            decoded.allowed_biomes,
            HolderSet::direct(vec![Holder::reference(owner, 0)])
        );
    }

    #[test]
    fn checkerboard_indexing_matches_java_floor_mod() {
        // bitShift = size + 2 = 4: index = floorMod((qx >> 4) + (qz >> 4), 2).
        let src = CheckerboardColumnBiomeSource::new(set(&[40, 54]), 2);
        let plains = Holder::direct(BiomeId::from_id(40));
        let sunflower_plains = Holder::direct(BiomeId::from_id(54));
        let sampler = crate::biome::climate::Climate::empty();
        for qx in -8i32..8 {
            for qz in -8i32..8 {
                // `Math.floorMod((qx >> bitShift) + (qz >> bitShift), size)`.
                let index = ((qx >> 4).wrapping_add(qz >> 4)).rem_euclid(2);
                let expected = if index == 0 {
                    plains.clone()
                } else {
                    sunflower_plains.clone()
                };
                assert_eq!(
                    src.get_noise_biome(qx, 0, qz, &sampler),
                    expected,
                    "qx={qx} qz={qz}"
                );
            }
        }
        // A quart step of 16 (bitShift) flips the parity.
        let q = 0;
        let a = src.get_noise_biome(q, 0, q, &sampler);
        let b = src.get_noise_biome(q + 16, 0, q, &sampler);
        assert_ne!(a, b);
    }

    #[test]
    fn shift_count_masking_matches_java_jls_15_19() {
        // bitShift = size + 2 is unbounded (size is Codec.intRange(0, 62)),
        // and JLS 15.19 masks an int shift count to the low 5 bits. Sizes >= 30
        // give bitShift >= 32 where a plain Rust `>>` would panic in debug; the
        // port must mask exactly like Java: size 62 -> bitShift 64 -> effective
        // `>> 0`, size 30 -> bitShift 32 -> effective `>> 0`, size 29 ->
        // bitShift 31.
        let plains = Holder::direct(BiomeId::from_id(40));
        let sunflower_plains = Holder::direct(BiomeId::from_id(54));
        let sampler = crate::biome::climate::Climate::empty();
        for (size, bit_shift) in [(62, 64), (30, 32), (29, 31)] {
            let src = CheckerboardColumnBiomeSource::new(set(&[40, 54]), size);
            assert_eq!(src.bit_shift, bit_shift);
            let masked = (bit_shift & 31) as u32;
            for qx in -8i32..8 {
                for qz in -8i32..8 {
                    // `Math.floorMod((qx >> (bitShift & 31)) + (qz >> (bitShift & 31)),
                    // allowedBiomes.size())`.
                    let index = (qx >> masked).wrapping_add(qz >> masked).rem_euclid(2);
                    let expected = if index == 0 {
                        plains.clone()
                    } else {
                        sunflower_plains.clone()
                    };
                    assert_eq!(
                        src.get_noise_biome(qx, 0, qz, &sampler),
                        expected,
                        "size={size} qx={qx} qz={qz}"
                    );
                }
            }
        }
    }

    #[test]
    fn dispatch_encodes_type() {
        let codec = biome_source_codec::<TestOps>();
        let (ops, owner) = biome_ops();
        let src: Arc<dyn BiomeSource> = Arc::new(CheckerboardColumnBiomeSource::new(
            HolderSet::direct(vec![Holder::reference(owner, 0)]),
            2,
        ));
        let encoded = codec
            .encode_start(&ops, &src)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "biomes": "minecraft:plains",
                "type": "minecraft:checkerboard"
            })
        );
    }

    fn map_codec_of<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn rivet_serialization::codec::Codec<CheckerboardColumnBiomeSource, Ops>> {
        rivet_serialization::map_codec::codec_of(CheckerboardColumnBiomeSource::map_codec::<Ops>())
    }
}
