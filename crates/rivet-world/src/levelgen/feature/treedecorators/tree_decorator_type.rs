//! Port of `net.minecraft.world.level.levelgen.feature.treedecorators.
//! TreeDecoratorType` (class, 26.2).
//!
//! Java is the per-decorator wrapper holding each `MapCodec<P>`; its ten
//! constants are `register(...)` calls into `BuiltInRegistries.
//! TREE_DECORATOR_TYPE`, each carrying the decorator's `MapCodec`, in this exact
//! declaration order. The Rust port mirrors `BlockStateProviderType`'s identity
//! split: the decorator's type identity is the opaque [`TreeDecoratorTypeId`]
//! handle (the registry element identity — element id == insertion index), and
//! the per-type `MapCodec`s are resolved by the dispatch table in
//! `tree_decorator`, not stored on the id.
//!
//! All ten Paper constants are declared with their exact registry identity and
//! declaration order (ids 0..=9), reproducing
//! `BuiltInRegistries.TREE_DECORATOR_TYPE`'s insertion order.

/// The `TreeDecoratorType<P>` registry element identity — the per-type `u32`
/// id (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `BlockStateProviderTypeId`. Identity-semantic (not
/// `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TreeDecoratorTypeId {
    /// The per-type `u32` identity (insertion index in the tree-decorator-type
    /// registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "trunk_vine", …)` → `minecraft:trunk_vine`).
    pub location: &'static str,
}

impl TreeDecoratorTypeId {
    /// `new TreeDecoratorTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> TreeDecoratorTypeId {
        TreeDecoratorTypeId { id, location }
    }

    /// `RegistryKey.getValue()` — the location as a string, the value the
    /// by-name codec encodes/decodes.
    pub fn location(&self) -> &'static str {
        self.location
    }
}

/// The ten `TreeDecoratorTypes` constants — Paper's exact declaration order in
/// `TreeDecoratorType.java` (the `BuiltInRegistries.TREE_DECORATOR_TYPE`
/// insertion order, so element ids 0..=9).
pub struct TreeDecoratorTypes;
impl TreeDecoratorTypes {
    /// `register("trunk_vine", TrunkVineDecorator.CODEC)`.
    pub const TRUNK_VINE: TreeDecoratorTypeId =
        TreeDecoratorTypeId::new(0, "minecraft:trunk_vine");
    /// `register("leave_vine", LeaveVineDecorator.CODEC)`.
    pub const LEAVE_VINE: TreeDecoratorTypeId =
        TreeDecoratorTypeId::new(1, "minecraft:leave_vine");
    /// `register("pale_moss", PaleMossDecorator.CODEC)`.
    pub const PALE_MOSS: TreeDecoratorTypeId =
        TreeDecoratorTypeId::new(2, "minecraft:pale_moss");
    /// `register("creaking_heart", CreakingHeartDecorator.CODEC)`.
    pub const CREAKING_HEART: TreeDecoratorTypeId =
        TreeDecoratorTypeId::new(3, "minecraft:creaking_heart");
    /// `register("cocoa", CocoaDecorator.CODEC)`.
    pub const COCOA: TreeDecoratorTypeId = TreeDecoratorTypeId::new(4, "minecraft:cocoa");
    /// `register("beehive", BeehiveDecorator.CODEC)`.
    pub const BEEHIVE: TreeDecoratorTypeId = TreeDecoratorTypeId::new(5, "minecraft:beehive");
    /// `register("alter_ground", AlterGroundDecorator.CODEC)`.
    pub const ALTER_GROUND: TreeDecoratorTypeId =
        TreeDecoratorTypeId::new(6, "minecraft:alter_ground");
    /// `register("attached_to_leaves", AttachedToLeavesDecorator.CODEC)`.
    pub const ATTACHED_TO_LEAVES: TreeDecoratorTypeId =
        TreeDecoratorTypeId::new(7, "minecraft:attached_to_leaves");
    /// `register("place_on_ground", PlaceOnGroundDecorator.CODEC)`.
    pub const PLACE_ON_GROUND: TreeDecoratorTypeId =
        TreeDecoratorTypeId::new(8, "minecraft:place_on_ground");
    /// `register("attached_to_logs", AttachedToLogsDecorator.CODEC)`.
    pub const ATTACHED_TO_LOGS: TreeDecoratorTypeId =
        TreeDecoratorTypeId::new(9, "minecraft:attached_to_logs");
}

/// `BuiltInRegistries.TREE_DECORATOR_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All ten Paper entries are registered,
/// so every known location resolves.
pub fn tree_decorator_type_by_name(name: &str) -> Option<TreeDecoratorTypeId> {
    match name {
        "minecraft:trunk_vine" => Some(TreeDecoratorTypes::TRUNK_VINE),
        "minecraft:leave_vine" => Some(TreeDecoratorTypes::LEAVE_VINE),
        "minecraft:pale_moss" => Some(TreeDecoratorTypes::PALE_MOSS),
        "minecraft:creaking_heart" => Some(TreeDecoratorTypes::CREAKING_HEART),
        "minecraft:cocoa" => Some(TreeDecoratorTypes::COCOA),
        "minecraft:beehive" => Some(TreeDecoratorTypes::BEEHIVE),
        "minecraft:alter_ground" => Some(TreeDecoratorTypes::ALTER_GROUND),
        "minecraft:attached_to_leaves" => Some(TreeDecoratorTypes::ATTACHED_TO_LEAVES),
        "minecraft:place_on_ground" => Some(TreeDecoratorTypes::PLACE_ON_GROUND),
        "minecraft:attached_to_logs" => Some(TreeDecoratorTypes::ATTACHED_TO_LOGS),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.TREE_DECORATOR_TYPE` element ids equal the
        // insertion index in `TreeDecoratorType.java`'s declaration order.
        assert_eq!(TreeDecoratorTypes::TRUNK_VINE.id, 0);
        assert_eq!(TreeDecoratorTypes::LEAVE_VINE.id, 1);
        assert_eq!(TreeDecoratorTypes::PALE_MOSS.id, 2);
        assert_eq!(TreeDecoratorTypes::CREAKING_HEART.id, 3);
        assert_eq!(TreeDecoratorTypes::COCOA.id, 4);
        assert_eq!(TreeDecoratorTypes::BEEHIVE.id, 5);
        assert_eq!(TreeDecoratorTypes::ALTER_GROUND.id, 6);
        assert_eq!(TreeDecoratorTypes::ATTACHED_TO_LEAVES.id, 7);
        assert_eq!(TreeDecoratorTypes::PLACE_ON_GROUND.id, 8);
        assert_eq!(TreeDecoratorTypes::ATTACHED_TO_LOGS.id, 9);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(TreeDecoratorTypes::TRUNK_VINE.location, "minecraft:trunk_vine");
        assert_eq!(TreeDecoratorTypes::LEAVE_VINE.location, "minecraft:leave_vine");
        assert_eq!(TreeDecoratorTypes::PALE_MOSS.location, "minecraft:pale_moss");
        assert_eq!(
            TreeDecoratorTypes::CREAKING_HEART.location,
            "minecraft:creaking_heart"
        );
        assert_eq!(TreeDecoratorTypes::COCOA.location, "minecraft:cocoa");
        assert_eq!(TreeDecoratorTypes::BEEHIVE.location, "minecraft:beehive");
        assert_eq!(TreeDecoratorTypes::ALTER_GROUND.location, "minecraft:alter_ground");
        assert_eq!(
            TreeDecoratorTypes::ATTACHED_TO_LEAVES.location,
            "minecraft:attached_to_leaves"
        );
        assert_eq!(
            TreeDecoratorTypes::PLACE_ON_GROUND.location,
            "minecraft:place_on_ground"
        );
        assert_eq!(
            TreeDecoratorTypes::ATTACHED_TO_LOGS.location,
            "minecraft:attached_to_logs"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        for id in [
            TreeDecoratorTypes::TRUNK_VINE,
            TreeDecoratorTypes::LEAVE_VINE,
            TreeDecoratorTypes::PALE_MOSS,
            TreeDecoratorTypes::CREAKING_HEART,
            TreeDecoratorTypes::COCOA,
            TreeDecoratorTypes::BEEHIVE,
            TreeDecoratorTypes::ALTER_GROUND,
            TreeDecoratorTypes::ATTACHED_TO_LEAVES,
            TreeDecoratorTypes::PLACE_ON_GROUND,
            TreeDecoratorTypes::ATTACHED_TO_LOGS,
        ] {
            assert_eq!(tree_decorator_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(tree_decorator_type_by_name("minecraft:nope"), None);
        assert_eq!(tree_decorator_type_by_name("trunk_vine"), None);
    }
}
