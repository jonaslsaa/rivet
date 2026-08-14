//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! TrunkPlacerType` (class, 26.2).
//!
//! Java is the per-placer wrapper holding each `MapCodec<P>`; its nine
//! constants are `register(...)` calls into `BuiltInRegistries.TRUNK_PLACER_TYPE`,
//! each carrying the placer's `MapCodec`, in this exact declaration order. The
//! Rust port mirrors `BlockStateProviderType`'s identity split: the placer's
//! type identity is the opaque [`TrunkPlacerTypeId`] handle (the registry
//! element identity — element id == insertion index), and the per-type
//! `MapCodec`s are resolved by the dispatch table in `trunk_placer`, not stored
//! on the id.
//!
//! All nine Paper constants are declared with their exact registry identity and
//! declaration order (ids 0..=8), reproducing
//! `BuiltInRegistries.TRUNK_PLACER_TYPE`'s insertion order.

/// The `TrunkPlacerType<P>` registry element identity — the per-type `u32` id
/// (element id == holder id == insertion index) plus its registry-key location,
/// mirroring `BlockStateProviderTypeId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrunkPlacerTypeId {
    /// The per-type `u32` identity (insertion index in the trunk-placer-type
    /// registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "straight_trunk_placer", …)` → `minecraft:straight_trunk_placer`).
    pub location: &'static str,
}

impl TrunkPlacerTypeId {
    /// `new TrunkPlacerTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> TrunkPlacerTypeId {
        TrunkPlacerTypeId { id, location }
    }

    /// `RegistryKey.getValue()` — the location as a string, the value the
    /// by-name codec encodes/decodes.
    pub fn location(&self) -> &'static str {
        self.location
    }
}

/// The nine `TrunkPlacerTypes` constants — Paper's exact declaration order in
/// `TrunkPlacerType.java` (the `BuiltInRegistries.TRUNK_PLACER_TYPE` insertion
/// order, so element ids 0..=8).
pub struct TrunkPlacerTypes;
impl TrunkPlacerTypes {
    /// `register("straight_trunk_placer", StraightTrunkPlacer.CODEC)`.
    pub const STRAIGHT_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(0, "minecraft:straight_trunk_placer");
    /// `register("forking_trunk_placer", ForkingTrunkPlacer.CODEC)`.
    pub const FORKING_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(1, "minecraft:forking_trunk_placer");
    /// `register("giant_trunk_placer", GiantTrunkPlacer.CODEC)`.
    pub const GIANT_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(2, "minecraft:giant_trunk_placer");
    /// `register("mega_jungle_trunk_placer", MegaJungleTrunkPlacer.CODEC)`.
    pub const MEGA_JUNGLE_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(3, "minecraft:mega_jungle_trunk_placer");
    /// `register("dark_oak_trunk_placer", DarkOakTrunkPlacer.CODEC)`.
    pub const DARK_OAK_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(4, "minecraft:dark_oak_trunk_placer");
    /// `register("fancy_trunk_placer", FancyTrunkPlacer.CODEC)`.
    pub const FANCY_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(5, "minecraft:fancy_trunk_placer");
    /// `register("bending_trunk_placer", BendingTrunkPlacer.CODEC)`.
    pub const BENDING_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(6, "minecraft:bending_trunk_placer");
    /// `register("upwards_branching_trunk_placer", UpwardsBranchingTrunkPlacer.CODEC)`.
    pub const UPWARDS_BRANCHING_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(7, "minecraft:upwards_branching_trunk_placer");
    /// `register("cherry_trunk_placer", CherryTrunkPlacer.CODEC)`.
    pub const CHERRY_TRUNK_PLACER: TrunkPlacerTypeId =
        TrunkPlacerTypeId::new(8, "minecraft:cherry_trunk_placer");
}

/// `BuiltInRegistries.TRUNK_PLACER_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All nine Paper entries are registered,
/// so every known location resolves.
pub fn trunk_placer_type_by_name(name: &str) -> Option<TrunkPlacerTypeId> {
    match name {
        "minecraft:straight_trunk_placer" => Some(TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER),
        "minecraft:forking_trunk_placer" => Some(TrunkPlacerTypes::FORKING_TRUNK_PLACER),
        "minecraft:giant_trunk_placer" => Some(TrunkPlacerTypes::GIANT_TRUNK_PLACER),
        "minecraft:mega_jungle_trunk_placer" => Some(TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER),
        "minecraft:dark_oak_trunk_placer" => Some(TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER),
        "minecraft:fancy_trunk_placer" => Some(TrunkPlacerTypes::FANCY_TRUNK_PLACER),
        "minecraft:bending_trunk_placer" => Some(TrunkPlacerTypes::BENDING_TRUNK_PLACER),
        "minecraft:upwards_branching_trunk_placer" => {
            Some(TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER)
        }
        "minecraft:cherry_trunk_placer" => Some(TrunkPlacerTypes::CHERRY_TRUNK_PLACER),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.TRUNK_PLACER_TYPE` element ids equal the
        // insertion index in `TrunkPlacerType.java`'s declaration order.
        assert_eq!(TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER.id, 0);
        assert_eq!(TrunkPlacerTypes::FORKING_TRUNK_PLACER.id, 1);
        assert_eq!(TrunkPlacerTypes::GIANT_TRUNK_PLACER.id, 2);
        assert_eq!(TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER.id, 3);
        assert_eq!(TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER.id, 4);
        assert_eq!(TrunkPlacerTypes::FANCY_TRUNK_PLACER.id, 5);
        assert_eq!(TrunkPlacerTypes::BENDING_TRUNK_PLACER.id, 6);
        assert_eq!(TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER.id, 7);
        assert_eq!(TrunkPlacerTypes::CHERRY_TRUNK_PLACER.id, 8);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(
            TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER.location,
            "minecraft:straight_trunk_placer"
        );
        assert_eq!(
            TrunkPlacerTypes::FORKING_TRUNK_PLACER.location,
            "minecraft:forking_trunk_placer"
        );
        assert_eq!(
            TrunkPlacerTypes::GIANT_TRUNK_PLACER.location,
            "minecraft:giant_trunk_placer"
        );
        assert_eq!(
            TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER.location,
            "minecraft:mega_jungle_trunk_placer"
        );
        assert_eq!(
            TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER.location,
            "minecraft:dark_oak_trunk_placer"
        );
        assert_eq!(
            TrunkPlacerTypes::FANCY_TRUNK_PLACER.location,
            "minecraft:fancy_trunk_placer"
        );
        assert_eq!(
            TrunkPlacerTypes::BENDING_TRUNK_PLACER.location,
            "minecraft:bending_trunk_placer"
        );
        assert_eq!(
            TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER.location,
            "minecraft:upwards_branching_trunk_placer"
        );
        assert_eq!(
            TrunkPlacerTypes::CHERRY_TRUNK_PLACER.location,
            "minecraft:cherry_trunk_placer"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        for id in [
            TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER,
            TrunkPlacerTypes::FORKING_TRUNK_PLACER,
            TrunkPlacerTypes::GIANT_TRUNK_PLACER,
            TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER,
            TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER,
            TrunkPlacerTypes::FANCY_TRUNK_PLACER,
            TrunkPlacerTypes::BENDING_TRUNK_PLACER,
            TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER,
            TrunkPlacerTypes::CHERRY_TRUNK_PLACER,
        ] {
            assert_eq!(trunk_placer_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(trunk_placer_type_by_name("minecraft:nope"), None);
        assert_eq!(trunk_placer_type_by_name("straight_trunk_placer"), None);
    }
}
