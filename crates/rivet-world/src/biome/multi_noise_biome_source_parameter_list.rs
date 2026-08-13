//! Port of `net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList`
//! (26.2) — the `mc.world.level.biome.source` unit.
//!
//! The preset registry element the `MultiNoiseBiomeSource.PRESET_CODEC` holder
//! resolves: a `Preset` plus the `ParameterList<Holder<Biome>>` the preset
//! builds through the biome `HolderGetter`. The `DIRECT_CODEC` is
//! `RecordCodecBuilder.create(...)` over `Preset.CODEC.fieldOf("preset")` and
//! `RegistryOps.retrieveGetter(Registries.BIOME)`; `CODEC` is the
//! `RegistryFileCodec` over the `MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST`
//! registry key.
//!
//! Translation notes:
//! - `Preset` is Java's generic `SourceProvider` — `apply(Function<ResourceKey<
//!   Biome>, T>)` builds a `ParameterList<T>` for any element type. The port
//!   specializes `PresetProvider` to the id element `Holder<BiomeId>` (the only
//!   element this unit's codecs/`parameters()` build). `used_biomes` recovers
//!   the distinct `ResourceKey`s by running the provider against a recording
//!   getter (Java's `e -> e` identity: the provider calls `getOrThrow` per
//!   parameter point, so the recorded keys ARE the `usedBiomes`).
//! - `BY_NAME` is the two-entry map `{nether, overworld}`; Java keys it by
//!   `Identifier`, the port by the identifier string (the `Identifier` is not
//!   `Hash`). `Preset.CODEC` is `Identifier.CODEC.flatXmap` with Java's exact
//!   `"Unknown preset: <name>"` error.
//! - The `OVERWORLD` preset applies the `.data`-owned
//!   `OverworldBiomeBuilder.add_biomes`, which emits the 7594-point overworld
//!   table from the generated `OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS`; the
//!   `NETHER` preset iterates the generated `NETHER_BIOME_SOURCE_PARAMETER_POINTS`.
//!   Both applications are infallible — each preset always builds a nonempty
//!   `ParameterList`. A biome registry missing one of the referenced keys panics
//!   in `get_or_throw` (`"Missing element ..."`), which is Java-faithful:
//!   `HolderGetter.getOrThrow` throws `IllegalStateException`, propagating out of
//!   `MultiNoiseBiomeSourceParameterList::new` in the same codec-decode and
//!   bootstrap paths.

use crate::biome::biome_source::keys;
use crate::biome::biomes::register_from_full_name;
use crate::biome::climate::{ParameterList, ParameterPoint};
use crate::biome::overworld_biome_builder::OverworldBiomeBuilder;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::generated::worldgen::NETHER_BIOME_SOURCE_PARAMETER_POINTS;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::{HolderGetter, RegistryGetter};
use rivet_registry::identifier::{Identifier, identifier_codec};
use rivet_registry::registry_file_codec::RegistryFileCodec;
use rivet_registry::registry_ops::{RegistryOpsLookup, retrieve_getter};
use rivet_registry::{ResourceKey, TagKey};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// `MultiNoiseBiomeSourceParameterList` — a `Preset` and its resolved parameter
/// list.
pub struct MultiNoiseBiomeSourceParameterList {
    /// `this.preset`.
    pub preset: Preset,
    /// `this.parameters` — `preset.provider.apply(biomes::getOrThrow)`.
    pub parameters: ParameterList<Holder<BiomeId>>,
}

impl std::fmt::Debug for MultiNoiseBiomeSourceParameterList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiNoiseBiomeSourceParameterList")
            .field("preset", &self.preset)
            .field("parameters", &self.parameters.values())
            .finish()
    }
}

impl Clone for MultiNoiseBiomeSourceParameterList {
    fn clone(&self) -> Self {
        MultiNoiseBiomeSourceParameterList {
            preset: self.preset.clone(),
            parameters: ParameterList::new(self.parameters.values().to_vec()),
        }
    }
}

impl PartialEq for MultiNoiseBiomeSourceParameterList {
    fn eq(&self, other: &Self) -> bool {
        // `ParameterList` carries an RTree index (no `PartialEq`); compare the
        // values, mirroring Java's record `equals` on the `values` list.
        self.preset == other.preset && self.parameters.values() == other.parameters.values()
    }
}

impl MultiNoiseBiomeSourceParameterList {
    /// `new MultiNoiseBiomeSourceParameterList(Preset, HolderGetter<Biome>)`.
    pub fn new(preset: Preset, biomes: &dyn HolderGetter<BiomeId>) -> Self {
        let parameters = preset.provider_biome_id().apply(biomes);
        MultiNoiseBiomeSourceParameterList { preset, parameters }
    }

    /// `MultiNoiseBiomeSourceParameterList.parameters()`.
    pub fn parameters(&self) -> &ParameterList<Holder<BiomeId>> {
        &self.parameters
    }

    /// `MultiNoiseBiomeSourceParameterList.DIRECT_CODEC` — the record codec
    /// over `"preset"` + the biome `RegistryOps.retrieveGetter`.
    ///
    /// The getter field is Java's context-only `retrieveGetter` — it has no
    /// stored value (encode is a no-op), so it cannot be a `record_builder`
    /// field (which requires a `Function<O, F>` getter). The port builds the
    /// record codec by hand over the two `MapCodec`s: decode pairs the
    /// `"preset"` field with the ops-resolved getter (Java `RecordCodecBuilder`
    /// `apply2`, preserving error accumulation); encode writes only the
    /// `"preset"` field.
    pub fn direct_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn MapCodec<MultiNoiseBiomeSourceParameterList, Ops>> {
        let preset_field = codec::field_of(Preset::codec::<Ops>(), "preset".to_string());
        let getter = retrieve_getter::<BiomeId, Ops>(&rivet_registry::registries::BIOME);
        Arc::new(MultiNoiseBiomeSourceParameterListDirectCodec {
            preset_field,
            getter,
            _marker: std::marker::PhantomData,
        })
    }

    /// `MultiNoiseBiomeSourceParameterList.CODEC` — the `RegistryFileCodec`
    /// over `Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST`.
    pub fn codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn Codec<Holder<MultiNoiseBiomeSourceParameterList>, Ops>> {
        Arc::new(RegistryFileCodec::create(
            &keys::MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST,
            rivet_serialization::map_codec::codec_of(Self::direct_codec::<Ops>()),
        ))
    }
}

/// `DIRECT_CODEC`'s manual record codec — `RecordCodecBuilder.create` over the
/// `"preset"` field and the context `retrieveGetter` (see [`direct_codec`]).
struct MultiNoiseBiomeSourceParameterListDirectCodec<Ops: DynamicOps + 'static> {
    preset_field: Arc<dyn MapCodec<Preset, Ops>>,
    getter: Arc<dyn MapCodec<RegistryGetter<BiomeId>, Ops>>,
    _marker: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug
    for MultiNoiseBiomeSourceParameterListDirectCodec<Ops>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MultiNoiseBiomeSourceParameterListDirectCodec")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops>
    for MultiNoiseBiomeSourceParameterListDirectCodec<Ops>
{
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.preset_field.keys(ops)
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<MultiNoiseBiomeSourceParameterList, Ops>
    for MultiNoiseBiomeSourceParameterListDirectCodec<Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &dyn MapLike<Ops::Output>,
    ) -> DataResult<MultiNoiseBiomeSourceParameterList> {
        let preset = self.preset_field.decode(ops, input);
        let getter = self.getter.decode(ops, input);
        // Java `RecordCodecBuilder.create(...).apply(i, MultiNoiseBiomeSourceParameterList::new)`
        // — `DataResult.ap2` over the `(Preset, HolderGetter<Biome>)` pair. The
        // constructor is infallible, so the applicative result is flattened with
        // the identity `flat_map`.
        preset
            .apply2(
                |p: &Preset, g: &RegistryGetter<BiomeId>| {
                    let parameters = p.provider_biome_id().apply(g);
                    DataResult::success(MultiNoiseBiomeSourceParameterList {
                        preset: p.clone(),
                        parameters,
                    })
                },
                getter,
            )
            .flat_map(|r| r)
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<MultiNoiseBiomeSourceParameterList, Ops>
    for MultiNoiseBiomeSourceParameterListDirectCodec<Ops>
{
    fn encode(
        &self,
        input: &MultiNoiseBiomeSourceParameterList,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.preset_field.encode(&input.preset, ops, prefix);
    }
}

impl<Ops: DynamicOps + 'static> MapCodec<MultiNoiseBiomeSourceParameterList, Ops>
    for MultiNoiseBiomeSourceParameterListDirectCodec<Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &dyn MapLike<Ops::Output>,
    ) -> DataResult<MultiNoiseBiomeSourceParameterList> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &MultiNoiseBiomeSourceParameterList,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix);
    }
}

/// `Preset` — the named parameter-list builder (Java's record with the
/// generic `SourceProvider`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    /// `Preset.id` — the `Identifier` (`nether`/`overworld`).
    pub id: Identifier,
    /// The `SourceProvider` — builds a `ParameterList<Holder<BiomeId>>`
    /// through the biome `HolderGetter` (Java's `Function<ResourceKey<Biome>,
    /// T>` applied to `biomes::getOrThrow`).
    provider: PresetProvider,
}

/// `Preset.SourceProvider` — `Function<ResourceKey<Biome>, T>` specialized to
/// the id element `Holder<BiomeId>` (the only element this unit builds).
#[derive(Clone)]
#[allow(clippy::type_complexity)] // Java's generic `SourceProvider` function type.
pub struct PresetProvider(
    Arc<dyn Fn(&dyn HolderGetter<BiomeId>) -> ParameterList<Holder<BiomeId>> + Send + Sync>,
);

impl std::fmt::Debug for PresetProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PresetProvider")
    }
}

impl PartialEq for PresetProvider {
    fn eq(&self, _other: &Self) -> bool {
        // Java's `SourceProvider` is an anonymous class — no `equals`.
        true
    }
}

impl Eq for PresetProvider {}

impl PresetProvider {
    /// Wrap a `Fn(&dyn HolderGetter<BiomeId>) -> ParameterList<Holder<BiomeId>>`.
    pub fn new(
        f: impl Fn(&dyn HolderGetter<BiomeId>) -> ParameterList<Holder<BiomeId>> + Send + Sync + 'static,
    ) -> Self {
        PresetProvider(Arc::new(f))
    }

    /// `Preset.SourceProvider.apply(Function<ResourceKey<Biome>, T>)` — apply
    /// with the biome getter (Java's `apply(biomes::getOrThrow)`).
    pub fn apply(&self, biomes: &dyn HolderGetter<BiomeId>) -> ParameterList<Holder<BiomeId>> {
        (self.0)(biomes)
    }
}

impl Preset {
    /// `new Preset(Identifier, SourceProvider)`.
    pub fn new(id: Identifier, provider: PresetProvider) -> Self {
        Preset { id, provider }
    }

    /// `Preset.id`.
    pub fn id(&self) -> &Identifier {
        &self.id
    }

    /// The `SourceProvider`.
    pub fn provider_biome_id(&self) -> &PresetProvider {
        &self.provider
    }

    /// `Preset.NETHER` — `Identifier.withDefaultNamespace("nether")` and the
    /// five-entry nether parameter list from the generated
    /// `NETHER_BIOME_SOURCE_PARAMETER_POINTS` (Paper `knownPresets()`, the
    /// builder's value order).
    pub fn nether() -> Preset {
        Preset::new(
            Identifier::with_default_namespace("nether"),
            PresetProvider::new(|biomes| {
                let mut builder = Vec::with_capacity(NETHER_BIOME_SOURCE_PARAMETER_POINTS.len());
                for generated in NETHER_BIOME_SOURCE_PARAMETER_POINTS {
                    let key = register_from_full_name(generated.biome);
                    builder.push((ParameterPoint::from(generated), biomes.get_or_throw(&key)));
                }
                ParameterList::new(builder)
            }),
        )
    }

    /// `Preset.OVERWORLD` — `Identifier.withDefaultNamespace("overworld")` and
    /// `generateOverworldBiomes(lookup)` through the `OverworldBiomeBuilder`.
    ///
    /// The builder emits the 7594-point overworld table from the generated
    /// `OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS` (`.data` unit), so the provider
    /// always builds a nonempty `ParameterList`.
    pub fn overworld() -> Preset {
        Preset::new(
            Identifier::with_default_namespace("overworld"),
            PresetProvider::new(|biomes| {
                let mut builder: Vec<(ParameterPoint, Holder<BiomeId>)> = Vec::new();
                let biome_builder = OverworldBiomeBuilder::new();
                biome_builder.add_biomes(&mut |(point, key): (
                    ParameterPoint,
                    ResourceKey<BiomeId>,
                )| {
                    builder.push((point, biomes.get_or_throw(&key)));
                });
                ParameterList::new(builder)
            }),
        )
    }

    /// `Preset.usedBiomes()` — the distinct biome `ResourceKey`s the provider
    /// references, in first-reference order.
    ///
    /// Java `provider.apply(e -> e).values().stream().map(Pair::getSecond)
    /// .distinct()` — the identity function returns each key unchanged, so the
    /// used set is the distinct keys the provider asks `getOrThrow` for. The
    /// port runs the provider against a recording getter and dedupes the
    /// recorded keys (the id element cannot carry the `ResourceKey` itself).
    pub fn used_biomes(&self) -> Vec<ResourceKey<BiomeId>> {
        let recorder = RecordingGetter::new();
        self.provider.apply(&recorder);
        let mut seen = Vec::new();
        for key in recorder.keys() {
            if !seen.contains(&key) {
                seen.push(key);
            }
        }
        seen
    }

    /// `Preset.CODEC` — `Identifier.CODEC.flatXmap` over `BY_NAME` with the
    /// exact `"Unknown preset: <name>"` error.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Preset, Ops>> {
        codec::flat_xmap(
            identifier_codec::<Ops>(),
            Arc::new(|name: &Identifier| {
                BY_NAME
                    .get(&name.to_string())
                    .cloned()
                    .map(DataResult::success)
                    .unwrap_or_else(|| DataResult::error(format!("Unknown preset: {}", name)))
            }),
            Arc::new(|preset: &Preset| DataResult::success(preset.id.clone())),
        )
    }
}

/// `Preset.BY_NAME` — the two-entry map `{nether, overworld}` keyed by the
/// identifier string (`Stream.of(NETHER, OVERWORLD).collect(toMap(id, p))`).
pub static BY_NAME: std::sync::LazyLock<HashMap<String, Preset>> = std::sync::LazyLock::new(|| {
    let mut map = HashMap::new();
    let nether = Preset::nether();
    let overworld = Preset::overworld();
    map.insert(nether.id.to_string(), nether);
    map.insert(overworld.id.to_string(), overworld);
    map
});

/// The recording `HolderGetter<BiomeId>` for [`Preset::used_biomes`] — records
/// every `get_or_throw` key in first-call order and returns a `Direct` placeholder
/// holder (the provider only reads through `getOrThrow`, so the returned values
/// are discarded; the recorded keys ARE the used biomes).
struct RecordingGetter {
    keys: RefCell<Vec<ResourceKey<BiomeId>>>,
}

impl RecordingGetter {
    fn new() -> Self {
        RecordingGetter {
            keys: RefCell::new(Vec::new()),
        }
    }

    fn keys(&self) -> Vec<ResourceKey<BiomeId>> {
        self.keys.borrow().clone()
    }
}

impl HolderGetter<BiomeId> for RecordingGetter {
    fn get(&self, key: &ResourceKey<BiomeId>) -> Option<Holder<BiomeId>> {
        self.keys.borrow_mut().push(key.clone());
        Some(Holder::direct(BiomeId::from_id(0)))
    }

    fn get_tag(&self, _tag: &TagKey<BiomeId>) -> Option<rivet_registry::HolderSet<BiomeId>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;

    /// A `HolderGetter<BiomeId>` that resolves the generated biome names to
    /// `Direct` id holders (the nether preset's keys).
    struct NameGetter;

    impl HolderGetter<BiomeId> for NameGetter {
        fn get(&self, key: &ResourceKey<BiomeId>) -> Option<Holder<BiomeId>> {
            BiomeId::from_name(&key.identifier().to_string()).map(Holder::direct)
        }
        fn get_tag(&self, _tag: &TagKey<BiomeId>) -> Option<rivet_registry::HolderSet<BiomeId>> {
            None
        }
    }

    #[test]
    fn nether_preset_builds_five_entries_in_declaration_order() {
        let preset = Preset::nether();
        assert_eq!(preset.id().to_string(), "minecraft:nether");
        let list = preset.provider_biome_id().apply(&NameGetter);
        assert_eq!(list.values().len(), 5);
        // Entry order and the first parameter point mirror Java exactly.
        let (p0, h0) = &list.values()[0];
        assert_eq!(
            *h0,
            Holder::direct(BiomeId::from_name("minecraft:nether_wastes").unwrap())
        );
        assert_eq!(p0.temperature.min, p0.temperature.max);
        assert_eq!(p0.offset, 0);
        // The warped-forest entry carries the 0.375F offset, quantized by the
        // 10000.0F factor: `(long)(0.375F * 10000.0F) == 3750`.
        let (p3, h3) = &list.values()[3];
        assert_eq!(
            *h3,
            Holder::direct(BiomeId::from_name("minecraft:warped_forest").unwrap())
        );
        assert_eq!(p3.offset, 3750);
    }

    #[test]
    fn used_biomes_returns_the_distinct_keys_in_order() {
        let preset = Preset::nether();
        let used = preset.used_biomes();
        let names: Vec<String> = used.iter().map(|k| k.identifier().to_string()).collect();
        assert_eq!(
            names,
            vec![
                "minecraft:nether_wastes",
                "minecraft:soul_sand_valley",
                "minecraft:crimson_forest",
                "minecraft:warped_forest",
                "minecraft:basalt_deltas",
            ]
        );
    }

    #[test]
    fn overworld_preset_application_builds_the_full_parameter_list() {
        // The `.data`-owned `OverworldBiomeBuilder::add_biomes` emits the
        // 7594-point overworld table, so applying the overworld provider builds
        // a nonempty `ParameterList` (Java `generateOverworldBiomes`).
        let preset = BY_NAME
            .get("minecraft:overworld")
            .expect("overworld preset present");
        assert_eq!(preset.id().to_string(), "minecraft:overworld");
        let list = preset.provider_biome_id().apply(&NameGetter);
        assert_eq!(list.values().len(), 7594);
        // `used_biomes` recovers the 55 distinct overworld biomes (the surface
        // set plus the underground dripstone/lush/sulfur/deep_dark), in
        // first-reference order.
        let used = preset.used_biomes();
        assert_eq!(used.len(), 55);
        assert_eq!(
            used[0].identifier().to_string(),
            "minecraft:mushroom_fields"
        );
        let names: Vec<String> = used.iter().map(|k| k.identifier().to_string()).collect();
        for expected in [
            "minecraft:mushroom_fields",
            "minecraft:plains",
            "minecraft:deep_dark",
            "minecraft:dripstone_caves",
            "minecraft:lush_caves",
            "minecraft:sulfur_caves",
        ] {
            assert!(names.contains(&expected.to_string()), "{expected} used");
        }
        let list = MultiNoiseBiomeSourceParameterList::new(preset.clone(), &NameGetter);
        assert_eq!(list.parameters().values().len(), 7594);
    }

    #[test]
    fn direct_codec_round_trips_a_direct_preset() {
        use crate::biome::biomes;
        use rivet_registry::biome_id::BiomeId;
        use rivet_registry::registry_ops::RegistryOps;
        use rivet_registry::{RegistrationInfo, RegistryAccess, RegistryBuilder};
        use rivet_serialization::json_ops::JsonOps;
        use serde_json::Value;
        use std::sync::Arc;

        type TestOps = RegistryOps<Value, JsonOps>;

        // A registry-backed ops with the five nether biome keys — the nether
        // provider `getOrThrow`s exactly these, and `retrieveGetter` resolves
        // the `RegistryGetter` through the ops.
        let key = rivet_registry::registries::BIOME.clone();
        let mut builder = RegistryBuilder::<BiomeId>::new(&key);
        for (i, k) in [
            &biomes::NETHER_WASTES,
            &biomes::SOUL_SAND_VALLEY,
            &biomes::CRIMSON_FOREST,
            &biomes::WARPED_FOREST,
            &biomes::BASALT_DELTAS,
        ]
        .iter()
        .enumerate()
        {
            builder.register(
                k,
                Arc::new(BiomeId::from_id(i as u16)),
                RegistrationInfo::BUILT_IN,
            );
        }
        let registry = builder.freeze();
        let access = RegistryAccess::from_single_registry(key, registry);
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);

        // The source is built through the same ops-backed getter, so both the
        // encoded and decoded lists carry the matching `Reference` holders.
        let getter = ops
            .getter(&rivet_registry::registries::BIOME)
            .expect("biome registry present");
        let list = MultiNoiseBiomeSourceParameterList::new(Preset::nether(), &getter);

        let codec = rivet_serialization::map_codec::codec_of(
            MultiNoiseBiomeSourceParameterList::direct_codec::<TestOps>(),
        );
        let encoded = codec
            .encode_start(&ops, &list)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, serde_json::json!({"preset": "minecraft:nether"}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, list);
    }

    #[test]
    fn preset_codec_round_trips_and_unknown_preset_errors() {
        let codec = Preset::codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &Preset::nether())
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, serde_json::json!("minecraft:nether"));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, Preset::nether());
        let unknown = codec
            .parse(&JsonOps::INSTANCE, &serde_json::json!("minecraft:the_end"))
            .error_ref()
            .map(|e| e.message().to_string());
        assert_eq!(
            unknown.as_deref(),
            Some("Unknown preset: minecraft:the_end")
        );
    }
}
