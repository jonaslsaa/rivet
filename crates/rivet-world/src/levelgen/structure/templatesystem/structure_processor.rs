//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessor`,
//! `StructureProcessorType`, `StructureProcessorTypes`, and `StructureProcessorList` (MC 26.2).

use super::processors::{
    BlackstoneReplaceProcessor, BlockAgeProcessor, BlockIgnoreProcessor, BlockRotProcessor,
    CappedProcessor, GravityProcessor, JigsawReplacementProcessor, LavaSubmergedBlockProcessor,
    NopProcessor, ProtectedBlockProcessor, RuleProcessor, blackstone_replace_map_codec,
    block_age_map_codec, block_ignore_map_codec, block_rot_map_codec, capped_map_codec,
    gravity_map_codec, jigsaw_replacement_map_codec, lava_submerged_block_map_codec, nop_map_codec,
    protected_block_map_codec, rule_processor_map_codec,
};
use super::structure_place_settings::StructurePlaceSettings;
use super::structure_template::StructureBlockInfo;
use crate::level::world_gen_level::WorldGenLevel;
use rivet_registry::core::BlockPos;
use rivet_registry::identifier::Identifier;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec::{self, MapCodec};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// Closed built-in identity for `BuiltInRegistries.STRUCTURE_PROCESSOR` (MC 26.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StructureProcessorTypeId {
    id: u32,
    name: &'static str,
}

impl StructureProcessorTypeId {
    pub const fn new(id: u32, name: &'static str) -> Self {
        Self { id, name }
    }

    pub const fn id(self) -> u32 {
        self.id
    }

    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Ordered `StructureProcessorTypes` registrations matching Paper 26.2 `StructureProcessorTypes.java`.
pub struct StructureProcessorTypes;

impl StructureProcessorTypes {
    pub const BLACKSTONE_REPLACE: StructureProcessorTypeId =
        StructureProcessorTypeId::new(0, "minecraft:blackstone_replace");
    pub const BLOCK_AGE: StructureProcessorTypeId =
        StructureProcessorTypeId::new(1, "minecraft:block_age");
    pub const BLOCK_IGNORE: StructureProcessorTypeId =
        StructureProcessorTypeId::new(2, "minecraft:block_ignore");
    pub const BLOCK_ROT: StructureProcessorTypeId =
        StructureProcessorTypeId::new(3, "minecraft:block_rot");
    pub const CAPPED: StructureProcessorTypeId =
        StructureProcessorTypeId::new(4, "minecraft:capped");
    pub const GRAVITY: StructureProcessorTypeId =
        StructureProcessorTypeId::new(5, "minecraft:gravity");
    pub const JIGSAW_REPLACEMENT: StructureProcessorTypeId =
        StructureProcessorTypeId::new(6, "minecraft:jigsaw_replacement");
    pub const LAVA_SUBMERGED_BLOCK: StructureProcessorTypeId =
        StructureProcessorTypeId::new(7, "minecraft:lava_submerged_block");
    pub const NOP: StructureProcessorTypeId = StructureProcessorTypeId::new(8, "minecraft:nop");
    pub const PROTECTED_BLOCKS: StructureProcessorTypeId =
        StructureProcessorTypeId::new(9, "minecraft:protected_blocks");
    pub const RULE: StructureProcessorTypeId = StructureProcessorTypeId::new(10, "minecraft:rule");

    pub const ALL: [StructureProcessorTypeId; 11] = [
        Self::BLACKSTONE_REPLACE,
        Self::BLOCK_AGE,
        Self::BLOCK_IGNORE,
        Self::BLOCK_ROT,
        Self::CAPPED,
        Self::GRAVITY,
        Self::JIGSAW_REPLACEMENT,
        Self::LAVA_SUBMERGED_BLOCK,
        Self::NOP,
        Self::PROTECTED_BLOCKS,
        Self::RULE,
    ];
}

pub fn structure_processor_type_by_name(name: &str) -> Option<StructureProcessorTypeId> {
    let canonical = if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    };
    StructureProcessorTypes::ALL
        .iter()
        .copied()
        .find(|id| id.name() == canonical)
}

/// `net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessor`.
pub trait StructureProcessor: Any + Debug + Send + Sync + 'static {
    /// `processBlock(...)` — returns `None` to drop the block from placement.
    fn process_block(
        &self,
        _level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        _template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        _settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        Some(processed_block_info)
    }

    /// `evaluatesEntirePieceState()` — returns `true` for whole-piece processors
    /// like `CappedProcessor`.
    fn evaluates_entire_piece_state(&self) -> bool {
        false
    }

    /// `finalizeProcessing(...)` — whole-piece post-pass executed after all
    /// per-block processors finish.
    fn finalize_processing(
        &self,
        _level: &mut dyn WorldGenLevel,
        _position: BlockPos,
        _reference_pos: BlockPos,
        _original_block_info_list: &[StructureBlockInfo],
        processed_block_info_list: Vec<StructureBlockInfo>,
        _settings: &StructurePlaceSettings,
    ) -> Vec<StructureBlockInfo> {
        processed_block_info_list
    }

    fn type_id(&self) -> StructureProcessorTypeId;

    fn as_any(&self) -> &dyn Any;
}

/// Erased carrier for `StructureProcessor` values stored in `StructurePlaceSettings`
/// and `StructureProcessorList`.
pub trait ErasedStructureProcessor: Any + Debug + Send + Sync + 'static {
    fn type_id(&self) -> StructureProcessorTypeId;
    fn as_any(&self) -> &dyn Any;
}

impl<T: StructureProcessor> ErasedStructureProcessor for T {
    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessor::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        StructureProcessor::as_any(self)
    }
}

/// `net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessorList`.
#[derive(Clone, Debug, Default)]
pub struct StructureProcessorList {
    list: Vec<Arc<dyn ErasedStructureProcessor>>,
}

impl StructureProcessorList {
    pub fn new(list: Vec<Arc<dyn ErasedStructureProcessor>>) -> Self {
        Self { list }
    }

    pub fn list(&self) -> &[Arc<dyn ErasedStructureProcessor>] {
        &self.list
    }
}

pub fn erased_evaluates_entire_piece_state(processor: &Arc<dyn ErasedStructureProcessor>) -> bool {
    match ErasedStructureProcessor::type_id(&**processor) {
        StructureProcessorTypes::CAPPED => processor
            .as_any()
            .downcast_ref::<CappedProcessor>()
            .expect("type_id/downcast mismatch for CappedProcessor")
            .evaluates_entire_piece_state(),
        _ => false,
    }
}

pub fn erased_process_block(
    processor: &Arc<dyn ErasedStructureProcessor>,
    level: &mut dyn WorldGenLevel,
    target_position: BlockPos,
    reference_pos: BlockPos,
    template_relative_pos: BlockPos,
    processed_block_info: StructureBlockInfo,
    settings: &StructurePlaceSettings,
) -> Option<StructureBlockInfo> {
    match ErasedStructureProcessor::type_id(&**processor) {
        StructureProcessorTypes::BLACKSTONE_REPLACE => processor
            .as_any()
            .downcast_ref::<BlackstoneReplaceProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::BLOCK_AGE => processor
            .as_any()
            .downcast_ref::<BlockAgeProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::BLOCK_IGNORE => processor
            .as_any()
            .downcast_ref::<BlockIgnoreProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::BLOCK_ROT => processor
            .as_any()
            .downcast_ref::<BlockRotProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::CAPPED => processor
            .as_any()
            .downcast_ref::<CappedProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::GRAVITY => processor
            .as_any()
            .downcast_ref::<GravityProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::JIGSAW_REPLACEMENT => processor
            .as_any()
            .downcast_ref::<JigsawReplacementProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::LAVA_SUBMERGED_BLOCK => processor
            .as_any()
            .downcast_ref::<LavaSubmergedBlockProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::NOP => processor
            .as_any()
            .downcast_ref::<NopProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::PROTECTED_BLOCKS => processor
            .as_any()
            .downcast_ref::<ProtectedBlockProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        StructureProcessorTypes::RULE => processor
            .as_any()
            .downcast_ref::<RuleProcessor>()
            .expect("downcast mismatch")
            .process_block(
                level,
                target_position,
                reference_pos,
                template_relative_pos,
                processed_block_info,
                settings,
            ),
        other => panic!("Unknown StructureProcessorTypeId: {other:?}"),
    }
}

pub fn erased_finalize_processing(
    processor: &Arc<dyn ErasedStructureProcessor>,
    level: &mut dyn WorldGenLevel,
    position: BlockPos,
    reference_pos: BlockPos,
    original_block_info_list: &[StructureBlockInfo],
    processed_block_info_list: Vec<StructureBlockInfo>,
    settings: &StructurePlaceSettings,
) -> Vec<StructureBlockInfo> {
    match ErasedStructureProcessor::type_id(&**processor) {
        StructureProcessorTypes::CAPPED => processor
            .as_any()
            .downcast_ref::<CappedProcessor>()
            .expect("downcast mismatch")
            .finalize_processing(
                level,
                position,
                reference_pos,
                original_block_info_list,
                processed_block_info_list,
                settings,
            ),
        _ => processed_block_info_list,
    }
}

/// `BuiltInRegistries.STRUCTURE_PROCESSOR.byNameCodec()`.
pub fn structure_processor_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<StructureProcessorTypeId, Ops>> {
    let string_codec = codec::string_codec::<Ops>();
    codec::flat_xmap(
        string_codec,
        Arc::new(|s: &String| {
            let parsed = Identifier::try_parse(s)
                .ok_or_else(|| format!("Not a valid resource location: {s}"));
            match parsed {
                Err(e) => DataResult::error(e),
                Ok(id) => {
                    let full = id.to_string();
                    match structure_processor_type_by_name(&full) {
                        Some(ty) => DataResult::success(ty),
                        None => DataResult::error(format!(
                            "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/structure_processor]: {full}"
                        )),
                    }
                }
            }
        }),
        Arc::new(|ty: &StructureProcessorTypeId| DataResult::success(ty.name().to_string())),
    )
}

fn lift_clone_processor_map_codec<T, Ops>(
    inner: Arc<dyn MapCodec<T, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedStructureProcessor>, Ops>>
where
    T: StructureProcessor + Clone,
    Ops: DynamicOps + 'static,
{
    map_codec::flat_xmap(
        inner,
        Arc::new(|v: &T| {
            DataResult::success(Arc::new(v.clone()) as Arc<dyn ErasedStructureProcessor>)
        }),
        Arc::new(|erased: &Arc<dyn ErasedStructureProcessor>| {
            match erased.as_any().downcast_ref::<T>() {
                Some(v) => DataResult::success(v.clone()),
                None => DataResult::error("Mismatched StructureProcessor runtime type".to_string()),
            }
        }),
    )
}

/// `StructureProcessorType.SINGLE_CODEC` — dispatches by `"processor_type"`,
/// using `codec::recursive` to support nested `CappedProcessor.delegate`.
pub fn structure_processor_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Arc<dyn ErasedStructureProcessor>, Ops>> {
    codec::recursive(
        "StructureProcessor".to_string(),
        Arc::new(
            |self_codec: Arc<dyn Codec<Arc<dyn ErasedStructureProcessor>, Ops>>| {
                let type_codec = structure_processor_type_by_name_codec::<Ops>();
                let map = key_dispatch_codec::dispatch_map(
                    "processor_type",
                    type_codec,
                    Arc::new(|p: &Arc<dyn ErasedStructureProcessor>| {
                        DataResult::success(ErasedStructureProcessor::type_id(&**p))
                    }),
                    Arc::new(move |id: &StructureProcessorTypeId| {
                        let codec: Arc<dyn MapCodec<Arc<dyn ErasedStructureProcessor>, Ops>> =
                            match *id {
                                StructureProcessorTypes::BLACKSTONE_REPLACE => {
                                    lift_clone_processor_map_codec(blackstone_replace_map_codec())
                                }
                                StructureProcessorTypes::BLOCK_AGE => {
                                    lift_clone_processor_map_codec(block_age_map_codec())
                                }
                                StructureProcessorTypes::BLOCK_IGNORE => {
                                    lift_clone_processor_map_codec(block_ignore_map_codec())
                                }
                                StructureProcessorTypes::BLOCK_ROT => {
                                    lift_clone_processor_map_codec(block_rot_map_codec())
                                }
                                StructureProcessorTypes::CAPPED => lift_clone_processor_map_codec(
                                    capped_map_codec(Arc::clone(&self_codec)),
                                ),
                                StructureProcessorTypes::GRAVITY => {
                                    lift_clone_processor_map_codec(gravity_map_codec())
                                }
                                StructureProcessorTypes::JIGSAW_REPLACEMENT => {
                                    lift_clone_processor_map_codec(jigsaw_replacement_map_codec())
                                }
                                StructureProcessorTypes::LAVA_SUBMERGED_BLOCK => {
                                    lift_clone_processor_map_codec(lava_submerged_block_map_codec())
                                }
                                StructureProcessorTypes::NOP => {
                                    lift_clone_processor_map_codec(nop_map_codec())
                                }
                                StructureProcessorTypes::PROTECTED_BLOCKS => {
                                    lift_clone_processor_map_codec(protected_block_map_codec())
                                }
                                StructureProcessorTypes::RULE => {
                                    lift_clone_processor_map_codec(rule_processor_map_codec())
                                }
                                other => {
                                    return DataResult::error(format!(
                                        "Unknown StructureProcessorType: {other:?}"
                                    ));
                                }
                            };
                        DataResult::success(codec)
                    }),
                );
                map_codec::codec_of(map)
            },
        ),
    )
}

/// `StructureProcessorType.LIST_OBJECT_CODEC`.
pub fn structure_processor_list_object_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<StructureProcessorList, Ops>> {
    let list_codec = codec::list(structure_processor_codec::<Ops>());
    codec::xmap(
        list_codec,
        Arc::new(|list: &Vec<Arc<dyn ErasedStructureProcessor>>| {
            StructureProcessorList::new(list.clone())
        }),
        Arc::new(|pl: &StructureProcessorList| pl.list.clone()),
    )
}

/// `StructureProcessorType.DIRECT_CODEC` — accepts either `{ "processors": [...] }`
/// or a bare `[...]` list (matching `Codec.withAlternative(LIST_OBJECT_CODEC.fieldOf("processors").codec(), LIST_OBJECT_CODEC)`).
pub fn structure_processor_direct_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<StructureProcessorList, Ops>> {
    let obj = map_codec::codec_of(codec::field_of(
        structure_processor_list_object_codec::<Ops>(),
        "processors".to_string(),
    ));
    let bare = structure_processor_list_object_codec::<Ops>();
    codec::with_alternative(obj, bare)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn paper_declaration_order_and_by_name_lookup() {
        let names: Vec<&str> = StructureProcessorTypes::ALL
            .iter()
            .map(|id| id.name())
            .collect();
        assert_eq!(
            names,
            vec![
                "minecraft:blackstone_replace",
                "minecraft:block_age",
                "minecraft:block_ignore",
                "minecraft:block_rot",
                "minecraft:capped",
                "minecraft:gravity",
                "minecraft:jigsaw_replacement",
                "minecraft:lava_submerged_block",
                "minecraft:nop",
                "minecraft:protected_blocks",
                "minecraft:rule",
            ]
        );
        for id in StructureProcessorTypes::ALL {
            assert_eq!(structure_processor_type_by_name(id.name()), Some(id));
        }
        assert_eq!(
            structure_processor_type_by_name("minecraft:nonexistent_processor"),
            None
        );
    }

    #[test]
    fn processor_codec_rejects_unknown_type() {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = structure_processor_codec::<RegistryOps<serde_json::Value, JsonOps>>();
        let bad = json!({ "processor_type": "minecraft:does_not_exist" });
        assert!(codec.decode(&ops, &bad).is_error());
    }

    #[test]
    fn direct_codec_accepts_both_object_and_bare_list() {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = structure_processor_direct_codec::<RegistryOps<serde_json::Value, JsonOps>>();

        let obj = json!({
            "processors": [
                { "processor_type": "minecraft:nop" },
                { "processor_type": "minecraft:jigsaw_replacement" }
            ]
        });
        let (decoded_obj, _) = codec
            .decode(&ops, &obj)
            .result()
            .expect("object form should decode")
            .clone();
        assert_eq!(decoded_obj.list().len(), 2);
        assert_eq!(
            ErasedStructureProcessor::type_id(&*decoded_obj.list()[0]),
            StructureProcessorTypes::NOP
        );
        assert_eq!(
            ErasedStructureProcessor::type_id(&*decoded_obj.list()[1]),
            StructureProcessorTypes::JIGSAW_REPLACEMENT
        );

        let bare = json!([
            { "processor_type": "minecraft:blackstone_replace" },
            { "processor_type": "minecraft:lava_submerged_block" }
        ]);
        let (decoded_bare, _) = codec
            .decode(&ops, &bare)
            .result()
            .expect("bare list form should decode")
            .clone();
        assert_eq!(decoded_bare.list().len(), 2);
        assert_eq!(
            ErasedStructureProcessor::type_id(&*decoded_bare.list()[0]),
            StructureProcessorTypes::BLACKSTONE_REPLACE
        );
        assert_eq!(
            ErasedStructureProcessor::type_id(&*decoded_bare.list()[1]),
            StructureProcessorTypes::LAVA_SUBMERGED_BLOCK
        );
    }
}
