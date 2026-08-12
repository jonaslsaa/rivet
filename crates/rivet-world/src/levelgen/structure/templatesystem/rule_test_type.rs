//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.RuleTestType`
//! (interface, 26.2).
//!
//! Java is the interface every rule test's `type()` returns; its six constants
//! are `register(...)` calls into `BuiltInRegistries.RULE_TEST` (each holding
//! the rule test's `MapCodec`), in this exact declaration order. The Rust port
//! mirrors `BlockPredicateType`/`PlacementModifierType`'s identity split: the
//! rule test's type identity is the opaque [`RuleTestTypeId`] handle (the
//! registry element identity — element id == insertion index), and the
//! per-type `MapCodec`s are resolved by the dispatch table in `rule_test`, not
//! stored on the id or a behavior trait.

/// The `RuleTestType<P>` registry element identity — the per-type `u32` id
/// (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `BlockPredicateTypeId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleTestTypeId {
    /// The per-type `u32` identity (insertion index in the rule-test-type registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "block_match", …)` → `minecraft:block_match`).
    pub location: &'static str,
}

impl RuleTestTypeId {
    /// `new RuleTestTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> RuleTestTypeId {
        RuleTestTypeId { id, location }
    }
}

/// The six `RuleTestTypes` constants — Paper's exact declaration order in
/// `RuleTestType.java` (the `BuiltInRegistries.RULE_TEST` insertion order, so
/// element ids 0..=5).
pub struct RuleTestTypes;
impl RuleTestTypes {
    /// `register("always_true", AlwaysTrueTest.CODEC)`.
    pub const ALWAYS_TRUE_TEST: RuleTestTypeId = RuleTestTypeId::new(0, "minecraft:always_true");
    /// `register("block_match", BlockMatchTest.CODEC)`.
    pub const BLOCK_TEST: RuleTestTypeId = RuleTestTypeId::new(1, "minecraft:block_match");
    /// `register("blockstate_match", BlockStateMatchTest.CODEC)`.
    pub const BLOCKSTATE_TEST: RuleTestTypeId =
        RuleTestTypeId::new(2, "minecraft:blockstate_match");
    /// `register("tag_match", TagMatchTest.CODEC)`.
    pub const TAG_TEST: RuleTestTypeId = RuleTestTypeId::new(3, "minecraft:tag_match");
    /// `register("random_block_match", RandomBlockMatchTest.CODEC)`.
    pub const RANDOM_BLOCK_TEST: RuleTestTypeId =
        RuleTestTypeId::new(4, "minecraft:random_block_match");
    /// `register("random_blockstate_match", RandomBlockStateMatchTest.CODEC)`.
    pub const RANDOM_BLOCKSTATE_TEST: RuleTestTypeId =
        RuleTestTypeId::new(5, "minecraft:random_blockstate_match");
}

/// `BuiltInRegistries.RULE_TEST.get(Identifier)` — resolve a registry-key
/// location to its type id. All six Paper entries are registered (matching
/// Java's `registerSimple`-populated registry).
pub fn rule_test_type_by_name(name: &str) -> Option<RuleTestTypeId> {
    match name {
        "minecraft:always_true" => Some(RuleTestTypes::ALWAYS_TRUE_TEST),
        "minecraft:block_match" => Some(RuleTestTypes::BLOCK_TEST),
        "minecraft:blockstate_match" => Some(RuleTestTypes::BLOCKSTATE_TEST),
        "minecraft:tag_match" => Some(RuleTestTypes::TAG_TEST),
        "minecraft:random_block_match" => Some(RuleTestTypes::RANDOM_BLOCK_TEST),
        "minecraft:random_blockstate_match" => Some(RuleTestTypes::RANDOM_BLOCKSTATE_TEST),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.RULE_TEST` element ids equal the insertion
        // index in `RuleTestType.java`'s declaration order.
        assert_eq!(RuleTestTypes::ALWAYS_TRUE_TEST.id, 0);
        assert_eq!(RuleTestTypes::BLOCK_TEST.id, 1);
        assert_eq!(RuleTestTypes::BLOCKSTATE_TEST.id, 2);
        assert_eq!(RuleTestTypes::TAG_TEST.id, 3);
        assert_eq!(RuleTestTypes::RANDOM_BLOCK_TEST.id, 4);
        assert_eq!(RuleTestTypes::RANDOM_BLOCKSTATE_TEST.id, 5);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(
            RuleTestTypes::ALWAYS_TRUE_TEST.location,
            "minecraft:always_true"
        );
        assert_eq!(RuleTestTypes::BLOCK_TEST.location, "minecraft:block_match");
        assert_eq!(
            RuleTestTypes::BLOCKSTATE_TEST.location,
            "minecraft:blockstate_match"
        );
        assert_eq!(RuleTestTypes::TAG_TEST.location, "minecraft:tag_match");
        assert_eq!(
            RuleTestTypes::RANDOM_BLOCK_TEST.location,
            "minecraft:random_block_match"
        );
        assert_eq!(
            RuleTestTypes::RANDOM_BLOCKSTATE_TEST.location,
            "minecraft:random_blockstate_match"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        for id in [
            RuleTestTypes::ALWAYS_TRUE_TEST,
            RuleTestTypes::BLOCK_TEST,
            RuleTestTypes::BLOCKSTATE_TEST,
            RuleTestTypes::TAG_TEST,
            RuleTestTypes::RANDOM_BLOCK_TEST,
            RuleTestTypes::RANDOM_BLOCKSTATE_TEST,
        ] {
            assert_eq!(rule_test_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(rule_test_type_by_name("minecraft:nope"), None);
        assert_eq!(rule_test_type_by_name("always_true"), None);
    }
}
