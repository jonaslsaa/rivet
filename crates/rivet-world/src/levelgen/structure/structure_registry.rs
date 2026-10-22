//! Complete pinned STRUCTURE/STRUCTURE_SET values. Registry presence is not
//! executable body support: every unported body refuses explicitly.
use super::{
    StructureDefinition, StructureError,
    placement::{StructurePlacement, structure_placement_codec},
    registry_keys::{STRUCTURE, STRUCTURE_SET},
    structure_settings::{JsonRegistryOps, StructureSettings, decode},
};
use rivet_registry::{
    Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey, TagKey,
    access::{LayeredRegistryAccess, RegistryLayer},
    holder::{Holder, HolderId},
    registry::RegistryKey,
    registry_file_codec::RegistryFixedCodec,
    registry_ops::RegistryOps,
    root::AnyBox,
};
use rivet_serialization::json_ops::JsonOps;
use serde_json::Value;
use std::sync::Arc;

pub use super::structure_body::{JigsawSettings, StructureBody};
pub use super::structure_settings::StructureDataError;

#[cfg(test)]
#[path = "structure_registry_test.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq)]
pub struct Structure {
    pub settings: StructureSettings,
    pub body: StructureBody,
}
impl Structure {
    pub fn native_definition(
        &self,
        key: &Identifier,
    ) -> Result<StructureDefinition, StructureError> {
        match self.body {
            StructureBody::Mineshaft { kind } => Ok(StructureDefinition::Mineshaft {
                registry_id: key.to_string(),
                terrain_adjustment: self.settings.terrain_adaptation,
                kind,
            }),
            StructureBody::BuriedTreasure => Ok(StructureDefinition::BuriedTreasure {
                registry_id: key.to_string(),
                terrain_adjustment: self.settings.terrain_adaptation,
            }),
            _ => Err(StructureError::UnsupportedStructure(format!(
                "{key} ({})",
                self.body.type_id()
            ))),
        }
    }
    pub fn parse(value: &Value, access: &RegistryAccess) -> Result<Self, StructureDataError> {
        let settings = StructureSettings::parse(value, access)?;
        let body = StructureBody::parse(value, &settings, access)?;
        Ok(Self { settings, body })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructureSelectionEntry {
    pub structure: Holder<Structure>,
    pub weight: i32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct StructureSet {
    pub structures: Vec<StructureSelectionEntry>,
    pub placement: StructurePlacement,
}
impl StructureSet {
    pub fn parse(value: &Value, access: &RegistryAccess) -> Result<Self, StructureDataError> {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
        let mut structures = Vec::new();
        for entry in value["structures"]
            .as_array()
            .ok_or_else(|| StructureDataError("structures must be an array".into()))?
        {
            let weight = entry["weight"]
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .filter(|v| *v > 0)
                .ok_or_else(|| {
                    StructureDataError("structure selection weight must be positive int".into())
                })?;
            #[allow(clippy::arc_with_non_send_sync)]
            let codec: Arc<
                dyn rivet_serialization::codec::Codec<Holder<Structure>, JsonRegistryOps>,
            > = Arc::new(RegistryFixedCodec::create(&STRUCTURE));
            let structure = decode(codec, &ops, &entry["structure"], "selected structure")?;
            structures.push(StructureSelectionEntry { structure, weight });
        }
        let placement = decode(
            structure_placement_codec(),
            &ops,
            &value["placement"],
            "structure placement",
        )?;
        Ok(Self {
            structures,
            placement,
        })
    }
}

/// Original resource JSON and actual live registration IDs, captured from
/// Paper0a99345. Includes all34structures,20sets and20boundstructuretags.
pub fn pinned_records() -> Result<Vec<Value>, StructureDataError> {
    include_str!("../../../../../tools/rivet-oracle/fixtures/structure-registry.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|e| StructureDataError(e.to_string())))
        .collect()
}

pub fn build_structure_access(base: &RegistryAccess) -> Result<RegistryAccess, StructureDataError> {
    if base.lookup(&STRUCTURE).is_some() || base.lookup(&STRUCTURE_SET).is_some() {
        return Err(StructureDataError(
            "structure registry is already installed; refusing identity replacement".into(),
        ));
    }
    let records = pinned_records()?;
    let structures: Vec<_> = records
        .iter()
        .filter(|v| v["kind"] == "structure")
        .collect();
    let sets: Vec<_> = records
        .iter()
        .filter(|v| v["kind"] == "structure_set")
        .collect();
    if structures.len() != 34 || sets.len() != 20 {
        return Err(StructureDataError(
            "incomplete pinned structure registry capture".into(),
        ));
    }
    let mut builder = RegistryBuilder::new(&STRUCTURE);
    for (index, row) in structures.iter().enumerate() {
        validate_id(row, index)?;
        let key = record_key(row)?;
        builder.register(
            &ResourceKey::create(&STRUCTURE, key),
            Arc::new(Structure::parse(&row["data"], base)?),
            RegistrationInfo::BUILT_IN,
        );
    }
    let mut tags = Vec::new();
    for row in records.iter().filter(|v| v["kind"] == "structure_tag") {
        let mut members = Vec::new();
        for member in row["members"]
            .as_array()
            .ok_or_else(|| StructureDataError("invalid structure tag".into()))?
        {
            let name = member
                .as_str()
                .ok_or_else(|| StructureDataError("tag member must be identifier".into()))?;
            let id = structures
                .iter()
                .position(|row| row["key"] == name)
                .ok_or_else(|| {
                    StructureDataError(format!("missing structure tag member {name}"))
                })?;
            members.push(HolderId(id as u32));
        }
        tags.push((TagKey::create(&STRUCTURE, record_key(row)?), members));
    }
    builder.bind_tags(tags);
    let structures = builder.freeze();
    let access = merge(
        base,
        RegistryAccess::from_pairs(vec![(erase(&STRUCTURE), Box::new(structures) as AnyBox)]),
    );
    let mut builder = RegistryBuilder::new(&STRUCTURE_SET);
    for (index, row) in sets.iter().enumerate() {
        validate_id(row, index)?;
        builder.register(
            &ResourceKey::create(&STRUCTURE_SET, record_key(row)?),
            Arc::new(StructureSet::parse(&row["data"], &access)?),
            RegistrationInfo::BUILT_IN,
        );
    }
    let sets = builder.freeze();
    // Exclusion holders form a registry graph: resolve every named edge against
    // the COMPLETE frozen set registry before exposing any authority to callers.
    for key in sets.key_set() {
        let set = sets.get_optional(&key).expect("registered set");
        let common = match &set.placement {
            StructurePlacement::RandomSpread(v) => &v.settings,
            StructurePlacement::ConcentricRings(v) => &v.settings,
        };
        if let Some(zone) = &common.exclusion_zone
            && sets.get_optional(&zone.other_set).is_none()
        {
            return Err(StructureDataError(format!(
                "unresolved exclusion structure set {}",
                zone.other_set
            )));
        }
    }
    Ok(merge(
        &access,
        RegistryAccess::from_pairs(vec![(erase(&STRUCTURE_SET), Box::new(sets) as AnyBox)]),
    ))
}

fn record_key(row: &Value) -> Result<Identifier, StructureDataError> {
    row["key"]
        .as_str()
        .and_then(Identifier::try_parse)
        .ok_or_else(|| StructureDataError("invalid captured registry key".into()))
}
fn validate_id(row: &Value, index: usize) -> Result<(), StructureDataError> {
    if row["id"].as_u64() != Some(index as u64) {
        return Err(StructureDataError(
            "captured registry IDs/order are not contiguous".into(),
        ));
    }
    Ok(())
}
fn erase<T>(key: &RegistryKey<T>) -> RegistryKey<()> {
    ResourceKey::create_registry_key(key.identifier().clone())
}
fn merge(base: &RegistryAccess, added: RegistryAccess) -> RegistryAccess {
    LayeredRegistryAccess::new(vec![RegistryLayer::Static, RegistryLayer::Worldgen])
        .replace_from(RegistryLayer::Static, std::slice::from_ref(base))
        .replace_from(RegistryLayer::Worldgen, &[added])
        .composite_access()
}
