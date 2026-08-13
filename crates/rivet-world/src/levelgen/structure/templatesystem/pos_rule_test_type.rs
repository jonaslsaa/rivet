//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.PosRuleTestType`
//! (interface, 26.2).
//!
//! Java is the interface every position rule test's `type()` returns; its
//! three constants are `register(...)` calls into
//! `BuiltInRegistries.POS_RULE_TEST` (each holding the position rule test's
//! `MapCodec`), in this exact declaration order. The Rust port mirrors
//! `RuleTestType`'s identity split: the position rule test's type identity is
//! the opaque [`PosRuleTestTypeId`] handle (the registry element identity —
//! element id == insertion index), and the per-type `MapCodec`s are resolved
//! by the dispatch table in `pos_rule_test`, not stored on the id or a
//! behavior trait.

/// The `PosRuleTestType<P>` registry element identity — the per-type `u32` id
/// (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `RuleTestTypeId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PosRuleTestTypeId {
    /// The per-type `u32` identity (insertion index in the position-rule-test-type registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "linear_pos", …)` → `minecraft:linear_pos`).
    pub location: &'static str,
}

impl PosRuleTestTypeId {
    /// `new PosRuleTestTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> PosRuleTestTypeId {
        PosRuleTestTypeId { id, location }
    }
}

/// The three `PosRuleTestTypes` constants — Paper's exact declaration order in
/// `PosRuleTestType.java` (the `BuiltInRegistries.POS_RULE_TEST` insertion
/// order, so element ids 0..=2).
pub struct PosRuleTestTypes;
impl PosRuleTestTypes {
    /// `register("always_true", PosAlwaysTrueTest.CODEC)`.
    pub const ALWAYS_TRUE_TEST: PosRuleTestTypeId =
        PosRuleTestTypeId::new(0, "minecraft:always_true");
    /// `register("linear_pos", LinearPosTest.CODEC)`.
    pub const LINEAR_POS_TEST: PosRuleTestTypeId =
        PosRuleTestTypeId::new(1, "minecraft:linear_pos");
    /// `register("axis_aligned_linear_pos", AxisAlignedLinearPosTest.CODEC)`.
    pub const AXIS_ALIGNED_LINEAR_POS_TEST: PosRuleTestTypeId =
        PosRuleTestTypeId::new(2, "minecraft:axis_aligned_linear_pos");
}

/// `BuiltInRegistries.POS_RULE_TEST.get(Identifier)` — resolve a registry-key
/// location to its type id. All three Paper entries are registered (matching
/// Java's `registerSimple`-populated registry).
pub fn pos_rule_test_type_by_name(name: &str) -> Option<PosRuleTestTypeId> {
    match name {
        "minecraft:always_true" => Some(PosRuleTestTypes::ALWAYS_TRUE_TEST),
        "minecraft:linear_pos" => Some(PosRuleTestTypes::LINEAR_POS_TEST),
        "minecraft:axis_aligned_linear_pos" => Some(PosRuleTestTypes::AXIS_ALIGNED_LINEAR_POS_TEST),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.POS_RULE_TEST` element ids equal the insertion
        // index in `PosRuleTestType.java`'s declaration order.
        assert_eq!(PosRuleTestTypes::ALWAYS_TRUE_TEST.id, 0);
        assert_eq!(PosRuleTestTypes::LINEAR_POS_TEST.id, 1);
        assert_eq!(PosRuleTestTypes::AXIS_ALIGNED_LINEAR_POS_TEST.id, 2);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(
            PosRuleTestTypes::ALWAYS_TRUE_TEST.location,
            "minecraft:always_true"
        );
        assert_eq!(
            PosRuleTestTypes::LINEAR_POS_TEST.location,
            "minecraft:linear_pos"
        );
        assert_eq!(
            PosRuleTestTypes::AXIS_ALIGNED_LINEAR_POS_TEST.location,
            "minecraft:axis_aligned_linear_pos"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        for id in [
            PosRuleTestTypes::ALWAYS_TRUE_TEST,
            PosRuleTestTypes::LINEAR_POS_TEST,
            PosRuleTestTypes::AXIS_ALIGNED_LINEAR_POS_TEST,
        ] {
            assert_eq!(pos_rule_test_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(pos_rule_test_type_by_name("minecraft:nope"), None);
        assert_eq!(pos_rule_test_type_by_name("always_true"), None);
    }
}
