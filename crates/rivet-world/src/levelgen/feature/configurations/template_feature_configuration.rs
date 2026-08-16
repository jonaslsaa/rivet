//! Port of `net.minecraft.world.level.levelgen.feature.configurations.TemplateFeatureConfiguration`
//! (record, 26.2).
//!
//! Java: a one-field record `record TemplateFeatureConfiguration(WeightedList<
//! TemplateEntry> templates)` whose `CODEC` is a `RecordCodecBuilder` over the
//! required `"templates"` field (`WeightedList.codec(TemplateEntry.CODEC)`).
//! The nested `TemplateEntry` record (`Identifier template, List<Rotation>
//! rotations`) has its own `CODEC` over the required `"id"` field
//! (`Identifier.CODEC`) and the `"rotations"` field
//! (`Rotation.CODEC.listOf().optionalFieldOf("rotations", List.of(Rotation.values()))`
//! — the NON-lenient with-default optional defaulting to every rotation), plus
//! the static `of(Identifier)` factory building all rotations. DFU `Codec<T>`
//! is `Codec<E, Ops>` in the port, so the static Java constants are exposed as
//! the ops-generic `template_feature_configuration_codec::<Ops>()` /
//! `template_entry_codec::<Ops>()` factories.
//!
//! [`rotation_codec`] is the `Rotation.CODEC` value (`StringRepresentable.
//! fromEnum(Rotation::values)` — a plain by-name string codec). The `Rotation`
//! value type lives in `rivet-registry` (issue #126's RivetTodo defers the
//! codec surface to rivet-protocol), so the codec is ported here in-module via
//! [`codec::string_resolver`], exactly like `direction_codec`'s string half but
//! WITHOUT the `or_compressed` id-resolution alternative (Java's `Rotation.CODEC`
//! is only the enum-string form; `Rotation.BY_ID`/`STREAM_CODEC` are separate
//! surfaces). Unknown names error with the DFU-exact `"Unknown element name:{name}"`.
//!
//! The record is value-semantic, so `PartialEq`/`Eq` are derived (the
//! `WeightedList` compares `totalWeight` + ordered items; `Rotation` is an
//! `Ord` enum).

use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_registry::Identifier;
use rivet_registry::core::Rotation;
use rivet_registry::identifier::identifier_codec;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::weighted::{WeightedList, weighted_list_codec};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.TemplateFeatureConfiguration`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateFeatureConfiguration {
    /// `templates` — the weighted template list.
    pub templates: WeightedList<TemplateEntry>,
}

/// `TemplateFeatureConfiguration.TemplateEntry` — one template id plus the
/// rotations it may be placed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateEntry {
    /// `TemplateEntry.template` — the structure template id.
    pub template: Identifier,
    /// `TemplateEntry.rotations` — the allowed rotations (default: all).
    pub rotations: Vec<Rotation>,
}

impl TemplateFeatureConfiguration {
    /// `new TemplateFeatureConfiguration(WeightedList<TemplateEntry>)` — the
    /// record constructor (the codec's `apply` function).
    pub fn new(templates: WeightedList<TemplateEntry>) -> Self {
        TemplateFeatureConfiguration { templates }
    }

    /// `TemplateFeatureConfiguration.templates()`.
    pub fn templates(&self) -> &WeightedList<TemplateEntry> {
        &self.templates
    }
}

impl TemplateEntry {
    /// `new TemplateEntry(Identifier, List<Rotation>)` — the nested record
    /// constructor (the codec's `apply` function).
    pub fn new(template: Identifier, rotations: Vec<Rotation>) -> Self {
        TemplateEntry {
            template,
            rotations,
        }
    }

    /// `TemplateEntry.of(Identifier)` — `new TemplateEntry(template,
    /// List.of(Rotation.values()))`.
    pub fn of(template: Identifier) -> Self {
        TemplateEntry::new(template, Rotation::VALUES.to_vec())
    }

    /// `TemplateEntry.template()`.
    pub fn template(&self) -> &Identifier {
        &self.template
    }

    /// `TemplateEntry.rotations()`.
    pub fn rotations(&self) -> &[Rotation] {
        &self.rotations
    }
}

/// `Rotation.CODEC` — `StringRepresentable.fromEnum(Rotation::values)`, as the
/// ops-generic `rotation_codec::<Ops>()` factory.
///
/// `Rotation` in `rivet-registry` carries the `get_serialized_name`/`VALUES`
/// surface this enum codec needs but does not implement
/// `string_representable::StringRepresentable` (the codec surface defers with
/// rivet-protocol under RivetTodo(#126)), so this is built via
/// [`codec::string_resolver`] with the exact `EnumCodec.decode` semantics: an
/// unknown name errors with `"Unknown element name:{name}"`, and encode writes
/// the serialized name.
pub fn rotation_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Rotation, Ops>> {
    codec::string_resolver(
        Arc::new(|r: &Rotation| Some(r.get_serialized_name().to_string())),
        Arc::new(|name: &String| {
            Rotation::VALUES
                .iter()
                .find(|r| r.get_serialized_name() == name)
                .copied()
        }),
    )
}

/// `TemplateEntry.CODEC` — a record codec over the required `"id"` field and
/// the with-default `"rotations"` optional field, as the ops-generic
/// `template_entry_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     Identifier.CODEC.fieldOf("id"),
///     Rotation.CODEC.listOf().optionalFieldOf("rotations", List.of(Rotation.values())))
///     .apply(i, TemplateEntry::new))
/// ```
pub fn template_entry_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<TemplateEntry, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|e: &TemplateEntry| e.template.clone()),
                codec::field_of(identifier_codec::<Ops>(), "id".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|e: &TemplateEntry| e.rotations.clone()),
                codec::optional_field_of::<Vec<Rotation>, Ops>(
                    "rotations",
                    codec::list(rotation_codec::<Ops>()),
                    Rotation::VALUES.to_vec(),
                ),
            ))
            .apply(
                instance,
                Arc::new(|template: Identifier, rotations: Vec<Rotation>| {
                    TemplateEntry::new(template, rotations)
                }),
            )
    })
}

/// `TemplateFeatureConfiguration.CODEC` — a record codec over the required
/// `"templates"` field (`WeightedList.codec(TemplateEntry.CODEC)`), as the
/// ops-generic `template_feature_configuration_codec::<Ops>()` factory.
pub fn template_feature_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<TemplateFeatureConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &TemplateFeatureConfiguration| c.templates.clone()),
                codec::field_of(
                    weighted_list_codec::<TemplateEntry, Ops>(template_entry_codec::<Ops>()),
                    "templates".to_string(),
                ),
            ))
            .apply(instance, Arc::new(TemplateFeatureConfiguration::new))
    })
}

impl FeatureConfiguration for TemplateFeatureConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn sample_entry() -> TemplateEntry {
        TemplateEntry::of(Identifier::parse("minecraft:igloo/top"))
    }

    #[test]
    fn rotation_codec_round_trips_all_values() {
        let codec = rotation_codec::<JsonOps>();
        for value in Rotation::VALUES {
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &value)
                .result()
                .expect("encode should succeed")
                .clone();
            assert_eq!(encoded, json!(value.get_serialized_name()));
            let result = codec.parse(&JsonOps::INSTANCE, &encoded);
            let decoded = result.result().expect("decode should succeed");
            assert_eq!(*decoded, value);
        }
    }

    #[test]
    fn rotation_codec_rejects_unknown_name() {
        // `EnumCodec.decode` — the DFU-exact message.
        let codec = rotation_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!("spin"));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Unknown element name:spin");
    }

    #[test]
    fn template_entry_codec_round_trip() {
        // `TemplateEntry.of(id)` encodes with all four rotations in enum order
        // (the field value equals the default, so `optionalFieldOf` keeps the
        // explicit list only when it differs — here it equals `values()`, so it
        // is OMITTED on encode; the decode side restores the default).
        let codec = template_entry_codec::<JsonOps>();
        let entry = sample_entry();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &entry)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"id": "minecraft:igloo/top"}));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, entry);
    }

    #[test]
    fn template_entry_codec_round_trips_explicit_rotations() {
        // A rotation list differing from the default is written explicitly.
        let codec = template_entry_codec::<JsonOps>();
        let entry = TemplateEntry::new(
            Identifier::parse("minecraft:igloo/top"),
            vec![Rotation::Clockwise90, Rotation::Counterclockwise90],
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &entry)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"id": "minecraft:igloo/top", "rotations": ["clockwise_90", "counterclockwise_90"]})
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, entry);
    }

    #[test]
    fn template_entry_codec_requires_id() {
        let codec = template_entry_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key id"), "got: {msg}");
    }

    #[test]
    fn template_entry_of_uses_all_rotations() {
        let entry = TemplateEntry::of(Identifier::parse("minecraft:igloo/top"));
        assert_eq!(entry.rotations, Rotation::VALUES);
        assert_eq!(*entry.template(), Identifier::parse("minecraft:igloo/top"));
    }

    #[test]
    fn codec_round_trip() {
        // Paper's structure features wrap one entry with weight 1, e.g.
        // `WeightedList.of(new TemplateEntry(id, List.of(Rotation.values())))`.
        let config = TemplateFeatureConfiguration::new(WeightedList::of_values(&[sample_entry()]));
        let codec = template_feature_configuration_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"templates": [{"data": {"id": "minecraft:igloo/top"}, "weight": 1}]})
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_requires_templates_field() {
        let codec = template_feature_configuration_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key templates"), "got: {msg}");
    }

    #[test]
    fn codec_rejects_negative_weight() {
        // `Weighted.codec` uses `ExtraCodecs.NON_NEGATIVE_INT` — a negative
        // weight fails decode with `"Value must be non-negative: {n}"`.
        let codec = template_feature_configuration_codec::<JsonOps>();
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({"templates": [{"data": {"id": "minecraft:igloo/top"}, "weight": -1}]}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("Value must be non-negative"), "got: {msg}");
    }
}
