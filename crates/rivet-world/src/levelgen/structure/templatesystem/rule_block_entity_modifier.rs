//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.rule.blockentity.*`
//! (MC 26.2): `RuleBlockEntityModifier`, `RuleBlockEntityModifierType`, `Clear`,
//! `Passthrough`, `AppendStatic`, and `AppendLoot`.
//!
//! `RuleBlockEntityModifier.CODEC` dispatches on `"type"` across the closed
//! `RULE_BLOCK_ENTITY_MODIFIER` registry in Paper's declaration order:
//! `clear`, `passthrough`, `append_static`, `append_loot`.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::decoder::Decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::encoder::Encoder;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// Registry-held identity of a `RuleBlockEntityModifierType<?>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleBlockEntityModifierTypeId {
    /// Zero-based registration index in `BuiltInRegistries.RULE_BLOCK_ENTITY_MODIFIER`.
    pub raw_id: usize,
    /// Namespaced key (`"minecraft:clear"`, etc.).
    pub name: &'static str,
}

impl RuleBlockEntityModifierTypeId {
    pub const fn new(raw_id: usize, name: &'static str) -> Self {
        Self { raw_id, name }
    }
}

/// Closed registry constants in Paper's exact `RuleBlockEntityModifierType` declaration order.
pub struct RuleBlockEntityModifierTypes;

impl RuleBlockEntityModifierTypes {
    pub const CLEAR: RuleBlockEntityModifierTypeId =
        RuleBlockEntityModifierTypeId::new(0, "minecraft:clear");
    pub const PASSTHROUGH: RuleBlockEntityModifierTypeId =
        RuleBlockEntityModifierTypeId::new(1, "minecraft:passthrough");
    pub const APPEND_STATIC: RuleBlockEntityModifierTypeId =
        RuleBlockEntityModifierTypeId::new(2, "minecraft:append_static");
    pub const APPEND_LOOT: RuleBlockEntityModifierTypeId =
        RuleBlockEntityModifierTypeId::new(3, "minecraft:append_loot");

    pub const ALL: [RuleBlockEntityModifierTypeId; 4] = [
        Self::CLEAR,
        Self::PASSTHROUGH,
        Self::APPEND_STATIC,
        Self::APPEND_LOOT,
    ];

    pub fn by_name(name: &str) -> Option<RuleBlockEntityModifierTypeId> {
        let parsed = Identifier::try_parse(name)?;
        let normalized = parsed.to_string();
        Self::ALL.into_iter().find(|t| t.name == normalized)
    }
}

/// `net.minecraft.world.level.levelgen.structure.templatesystem.rule.blockentity.RuleBlockEntityModifier`.
pub trait RuleBlockEntityModifier: Any + Debug + Send + Sync + 'static {
    /// `apply(RandomSource, @Nullable CompoundTag)`.
    fn apply<R: RandomSource>(
        &self,
        random: &mut R,
        existing_tag: Option<&CompoundTag>,
    ) -> Option<CompoundTag>;

    fn type_id(&self) -> RuleBlockEntityModifierTypeId;

    fn as_any(&self) -> &dyn Any;
}

/// Object-safe erased carrier for `RuleBlockEntityModifier`.
pub trait ErasedRuleBlockEntityModifier: Any + Debug + Send + Sync + 'static {
    fn type_id(&self) -> RuleBlockEntityModifierTypeId;
    fn as_any(&self) -> &dyn Any;
}

impl<T: RuleBlockEntityModifier + ?Sized> ErasedRuleBlockEntityModifier for T {
    fn type_id(&self) -> RuleBlockEntityModifierTypeId {
        RuleBlockEntityModifier::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        RuleBlockEntityModifier::as_any(self)
    }
}

/// `Clear` — clears the block entity NBT (returns `None`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clear;

impl RuleBlockEntityModifier for Clear {
    fn apply<R: RandomSource>(
        &self,
        _random: &mut R,
        _existing_tag: Option<&CompoundTag>,
    ) -> Option<CompoundTag> {
        None
    }

    fn type_id(&self) -> RuleBlockEntityModifierTypeId {
        RuleBlockEntityModifierTypes::CLEAR
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `Passthrough` — preserves the existing block entity NBT unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Passthrough;

impl RuleBlockEntityModifier for Passthrough {
    fn apply<R: RandomSource>(
        &self,
        _random: &mut R,
        existing_tag: Option<&CompoundTag>,
    ) -> Option<CompoundTag> {
        existing_tag.cloned()
    }

    fn type_id(&self) -> RuleBlockEntityModifierTypeId {
        RuleBlockEntityModifierTypes::PASSTHROUGH
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `AppendStatic` — merges a static `CompoundTag` (`"data"`) onto the existing NBT.
#[derive(Clone, Debug, PartialEq)]
pub struct AppendStatic {
    pub tag: CompoundTag,
}

impl AppendStatic {
    pub fn new(tag: CompoundTag) -> Self {
        Self { tag }
    }
}

impl RuleBlockEntityModifier for AppendStatic {
    fn apply<R: RandomSource>(
        &self,
        _random: &mut R,
        existing_tag: Option<&CompoundTag>,
    ) -> Option<CompoundTag> {
        match existing_tag {
            None => Some(self.tag.clone()),
            Some(existing) => {
                let mut merged = existing.clone();
                merged.merge(&self.tag);
                Some(merged)
            }
        }
    }

    fn type_id(&self) -> RuleBlockEntityModifierTypeId {
        RuleBlockEntityModifierTypes::APPEND_STATIC
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `AppendLoot` — writes `"LootTable"` and `"LootTableSeed"` (`random.nextLong()`)
/// onto a copy of the existing NBT (or a new `CompoundTag` if none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppendLoot {
    pub loot_table: Identifier,
}

impl AppendLoot {
    pub fn new(loot_table: Identifier) -> Self {
        Self { loot_table }
    }
}

impl RuleBlockEntityModifier for AppendLoot {
    fn apply<R: RandomSource>(
        &self,
        random: &mut R,
        existing_tag: Option<&CompoundTag>,
    ) -> Option<CompoundTag> {
        let mut result = existing_tag.cloned().unwrap_or_default();
        result.put_string("LootTable", &self.loot_table.to_string());
        result.put_long("LootTableSeed", random.next_long());
        Some(result)
    }

    fn type_id(&self) -> RuleBlockEntityModifierTypeId {
        RuleBlockEntityModifierTypes::APPEND_LOOT
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Evaluate an erased `RuleBlockEntityModifier`.
pub fn erased_apply<R: RandomSource>(
    modifier: &Arc<dyn ErasedRuleBlockEntityModifier>,
    random: &mut R,
    existing_tag: Option<&CompoundTag>,
) -> Option<CompoundTag> {
    if let Some(m) = modifier.as_any().downcast_ref::<Clear>() {
        RuleBlockEntityModifier::apply(m, random, existing_tag)
    } else if let Some(m) = modifier.as_any().downcast_ref::<Passthrough>() {
        RuleBlockEntityModifier::apply(m, random, existing_tag)
    } else if let Some(m) = modifier.as_any().downcast_ref::<AppendStatic>() {
        RuleBlockEntityModifier::apply(m, random, existing_tag)
    } else if let Some(m) = modifier.as_any().downcast_ref::<AppendLoot>() {
        RuleBlockEntityModifier::apply(m, random, existing_tag)
    } else {
        panic!(
            "unregistered RuleBlockEntityModifier implementation: {:?}",
            modifier.type_id()
        )
    }
}

/// `CompoundTag` codec lifted to an arbitrary `DynamicOps` via `DynamicOps::convert_to`.
pub fn compound_tag_ops_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<CompoundTag, Ops>> {
    #[derive(Debug)]
    struct CompoundTagOpsCodec;

    impl<Ops: DynamicOps + 'static> Decoder<CompoundTag, Ops> for CompoundTagOpsCodec {
        fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(CompoundTag, Ops::Output)> {
            let nbt_tag = ops.convert_to(&NbtOps::instance(), input);
            match nbt_tag {
                Tag::Compound(c) => DataResult::success((c, ops.empty())),
                other => DataResult::error(format!("Not a compound tag: {other:?}")),
            }
        }
    }

    impl<Ops: DynamicOps + 'static> Encoder<CompoundTag, Ops> for CompoundTagOpsCodec {
        fn encode(
            &self,
            input: &CompoundTag,
            ops: &Ops,
            _prefix: &Ops::Output,
        ) -> DataResult<Ops::Output> {
            let converted = NbtOps::instance().convert_to(ops, &Tag::Compound(input.copy_tag()));
            DataResult::success(converted)
        }
    }

    impl<Ops: DynamicOps + 'static> Codec<CompoundTag, Ops> for CompoundTagOpsCodec {}

    Arc::new(CompoundTagOpsCodec)
}

pub fn clear_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<Clear, Ops>> {
    map_codec::unit(Clear)
}

pub fn passthrough_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<Passthrough, Ops>> {
    map_codec::unit(Passthrough)
}

pub fn append_static_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<AppendStatic, Ops>>
{
    let field = codec::field_of(compound_tag_ops_codec::<Ops>(), "data".to_string());
    map_codec::xmap(
        field,
        Arc::new(|tag: &CompoundTag| AppendStatic::new(tag.clone())),
        Arc::new(|m: &AppendStatic| m.tag.clone()),
    )
}

pub fn append_loot_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<AppendLoot, Ops>> {
    let field = codec::field_of(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        "loot_table".to_string(),
    );
    map_codec::xmap(
        field,
        Arc::new(|id: &Identifier| AppendLoot::new(id.clone())),
        Arc::new(|m: &AppendLoot| m.loot_table.clone()),
    )
}

pub fn rule_block_entity_modifier_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<RuleBlockEntityModifierTypeId, Ops>> {
    codec::comap_flat_map(
        codec::string_codec::<Ops>(),
        Arc::new(
            |name: &String| match RuleBlockEntityModifierTypes::by_name(name) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:rule_block_entity_modifier]: {name}"
                )),
            },
        ),
        Arc::new(|id: &RuleBlockEntityModifierTypeId| id.name.to_string()),
    )
}

fn lift_modifier_map_codec<T, Ops>(
    inner: Arc<dyn MapCodec<T, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedRuleBlockEntityModifier>, Ops>>
where
    T: RuleBlockEntityModifier + Clone,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(|v: &T| Arc::new(v.clone()) as Arc<dyn ErasedRuleBlockEntityModifier>),
        Arc::new(|erased: &Arc<dyn ErasedRuleBlockEntityModifier>| {
            erased
                .as_any()
                .downcast_ref::<T>()
                .expect("RuleBlockEntityModifier dispatch type_id matched concrete codec")
                .clone()
        }),
    )
}

/// `RuleBlockEntityModifier.CODEC` — dispatches on `"type"`.
pub fn rule_block_entity_modifier_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<Arc<dyn ErasedRuleBlockEntityModifier>, Ops>> {
    let type_codec = rule_block_entity_modifier_type_by_name_codec::<Ops>();
    let map = key_dispatch_codec::dispatch_map(
        "type",
        type_codec,
        Arc::new(|m: &Arc<dyn ErasedRuleBlockEntityModifier>| {
            DataResult::success(ErasedRuleBlockEntityModifier::type_id(&**m))
        }),
        Arc::new(|id: &RuleBlockEntityModifierTypeId| {
            let codec: Arc<dyn MapCodec<Arc<dyn ErasedRuleBlockEntityModifier>, Ops>> = match *id {
                RuleBlockEntityModifierTypes::CLEAR => lift_modifier_map_codec(clear_map_codec()),
                RuleBlockEntityModifierTypes::PASSTHROUGH => {
                    lift_modifier_map_codec(passthrough_map_codec())
                }
                RuleBlockEntityModifierTypes::APPEND_STATIC => {
                    lift_modifier_map_codec(append_static_map_codec())
                }
                RuleBlockEntityModifierTypes::APPEND_LOOT => {
                    lift_modifier_map_codec(append_loot_map_codec())
                }
                other => {
                    return DataResult::error(format!(
                        "Unknown RuleBlockEntityModifierType: {other:?}"
                    ));
                }
            };
            DataResult::success(codec)
        }),
    );
    map_codec::codec_of(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    #[test]
    fn clear_and_passthrough_behavior() {
        let mut rng = LegacyRandomSource::new(42);
        let mut existing = CompoundTag::new();
        existing.put_int("Count", 3);

        assert_eq!(Clear.apply(&mut rng, Some(&existing)), None);
        assert_eq!(
            Passthrough.apply(&mut rng, Some(&existing)),
            Some(existing.clone())
        );
        assert_eq!(Passthrough.apply(&mut rng, None), None);
    }

    #[test]
    fn append_static_merges_over_existing() {
        let mut rng = LegacyRandomSource::new(1);
        let mut existing = CompoundTag::new();
        existing.put_int("a", 1);
        existing.put_int("b", 2);

        let mut extra = CompoundTag::new();
        extra.put_int("b", 20);
        extra.put_int("c", 30);

        let modifier = AppendStatic::new(extra);
        let merged = modifier.apply(&mut rng, Some(&existing)).unwrap();
        assert_eq!(merged.get_int("a"), Some(1));
        assert_eq!(merged.get_int("b"), Some(20));
        assert_eq!(merged.get_int("c"), Some(30));
    }

    #[test]
    fn append_loot_writes_deterministic_seed() {
        let mut rng_a = LegacyRandomSource::new(12345);
        let mut rng_b = LegacyRandomSource::new(12345);
        let expected_seed = rng_b.next_long();

        let modifier = AppendLoot::new(Identifier::parse("minecraft:chests/simple_dungeon"));
        let out = modifier.apply(&mut rng_a, None).unwrap();
        assert_eq!(
            out.get_string("LootTable").map(String::as_str),
            Some("minecraft:chests/simple_dungeon")
        );
        assert_eq!(out.get_long("LootTableSeed"), Some(expected_seed));
    }

    #[test]
    fn dispatch_codec_round_trips_all_variants() {
        let codec = rule_block_entity_modifier_codec::<JsonOps>();

        let clear_json = json!({"type": "minecraft:clear"});
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &clear_json)
            .result()
            .unwrap()
            .clone();
        assert_eq!(
            ErasedRuleBlockEntityModifier::type_id(&*decoded),
            RuleBlockEntityModifierTypes::CLEAR
        );

        let loot_json = json!({
            "type": "minecraft:append_loot",
            "loot_table": "minecraft:chests/buried_treasure"
        });
        let decoded_loot = codec
            .parse(&JsonOps::INSTANCE, &loot_json)
            .result()
            .unwrap()
            .clone();
        assert_eq!(
            ErasedRuleBlockEntityModifier::type_id(&*decoded_loot),
            RuleBlockEntityModifierTypes::APPEND_LOOT
        );
        let reencoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded_loot)
            .result()
            .unwrap()
            .clone();
        assert_eq!(reencoded, loot_json);

        let unknown = json!({"type": "minecraft:does_not_exist"});
        assert!(codec.parse(&JsonOps::INSTANCE, &unknown).is_error());
    }
}
