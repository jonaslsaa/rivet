//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! FoliagePlacerType` (class, 26.2).
//!
//! Java is the per-placer wrapper holding each `MapCodec<P>`; its eleven
//! constants are `register(...)` calls into `BuiltInRegistries.
//! FOLIAGE_PLACER_TYPE`, each carrying the placer's `MapCodec`, in this exact
//! declaration order. The Rust port mirrors `BlockStateProviderType`'s identity
//! split: the placer's type identity is the opaque [`FoliagePlacerTypeId`]
//! handle (the registry element identity — element id == insertion index), and
//! the per-type `MapCodec`s are resolved by the dispatch table in
//! `foliage_placer`, not stored on the id.
//!
//! All eleven Paper constants are declared with their exact registry identity
//! and declaration order (ids 0..=10), reproducing
//! `BuiltInRegistries.FOLIAGE_PLACER_TYPE`'s insertion order.

/// The `FoliagePlacerType<P>` registry element identity — the per-type `u32`
/// id (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `BlockStateProviderTypeId`. Identity-semantic (not
/// `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FoliagePlacerTypeId {
    /// The per-type `u32` identity (insertion index in the foliage-placer-type
    /// registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "blob_foliage_placer", …)` → `minecraft:blob_foliage_placer`).
    pub location: &'static str,
}

impl FoliagePlacerTypeId {
    /// `new FoliagePlacerTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> FoliagePlacerTypeId {
        FoliagePlacerTypeId { id, location }
    }

    /// `RegistryKey.getValue()` — the location as a string, the value the
    /// by-name codec encodes/decodes.
    pub fn location(&self) -> &'static str {
        self.location
    }
}

/// The eleven `FoliagePlacerTypes` constants — Paper's exact declaration order
/// in `FoliagePlacerType.java` (the
/// `BuiltInRegistries.FOLIAGE_PLACER_TYPE` insertion order, so element ids
/// 0..=10).
pub struct FoliagePlacerTypes;
impl FoliagePlacerTypes {
    /// `register("blob_foliage_placer", BlobFoliagePlacer.CODEC)`.
    pub const BLOB_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(0, "minecraft:blob_foliage_placer");
    /// `register("spruce_foliage_placer", SpruceFoliagePlacer.CODEC)`.
    pub const SPRUCE_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(1, "minecraft:spruce_foliage_placer");
    /// `register("pine_foliage_placer", PineFoliagePlacer.CODEC)`.
    pub const PINE_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(2, "minecraft:pine_foliage_placer");
    /// `register("acacia_foliage_placer", AcaciaFoliagePlacer.CODEC)`.
    pub const ACACIA_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(3, "minecraft:acacia_foliage_placer");
    /// `register("bush_foliage_placer", BushFoliagePlacer.CODEC)`.
    pub const BUSH_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(4, "minecraft:bush_foliage_placer");
    /// `register("fancy_foliage_placer", FancyFoliagePlacer.CODEC)`.
    pub const FANCY_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(5, "minecraft:fancy_foliage_placer");
    /// `register("jungle_foliage_placer", MegaJungleFoliagePlacer.CODEC)`.
    pub const JUNGLE_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(6, "minecraft:jungle_foliage_placer");
    /// `register("mega_pine_foliage_placer", MegaPineFoliagePlacer.CODEC)`.
    pub const MEGA_PINE_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(7, "minecraft:mega_pine_foliage_placer");
    /// `register("dark_oak_foliage_placer", DarkOakFoliagePlacer.CODEC)`.
    pub const DARK_OAK_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(8, "minecraft:dark_oak_foliage_placer");
    /// `register("random_spread_foliage_placer", RandomSpreadFoliagePlacer.CODEC)`.
    pub const RANDOM_SPREAD_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(9, "minecraft:random_spread_foliage_placer");
    /// `register("cherry_foliage_placer", CherryFoliagePlacer.CODEC)`.
    pub const CHERRY_FOLIAGE_PLACER: FoliagePlacerTypeId =
        FoliagePlacerTypeId::new(10, "minecraft:cherry_foliage_placer");
}

/// `BuiltInRegistries.FOLIAGE_PLACER_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All eleven Paper entries are
/// registered, so every known location resolves.
pub fn foliage_placer_type_by_name(name: &str) -> Option<FoliagePlacerTypeId> {
    match name {
        "minecraft:blob_foliage_placer" => Some(FoliagePlacerTypes::BLOB_FOLIAGE_PLACER),
        "minecraft:spruce_foliage_placer" => Some(FoliagePlacerTypes::SPRUCE_FOLIAGE_PLACER),
        "minecraft:pine_foliage_placer" => Some(FoliagePlacerTypes::PINE_FOLIAGE_PLACER),
        "minecraft:acacia_foliage_placer" => Some(FoliagePlacerTypes::ACACIA_FOLIAGE_PLACER),
        "minecraft:bush_foliage_placer" => Some(FoliagePlacerTypes::BUSH_FOLIAGE_PLACER),
        "minecraft:fancy_foliage_placer" => Some(FoliagePlacerTypes::FANCY_FOLIAGE_PLACER),
        "minecraft:jungle_foliage_placer" => Some(FoliagePlacerTypes::JUNGLE_FOLIAGE_PLACER),
        "minecraft:mega_pine_foliage_placer" => Some(FoliagePlacerTypes::MEGA_PINE_FOLIAGE_PLACER),
        "minecraft:dark_oak_foliage_placer" => Some(FoliagePlacerTypes::DARK_OAK_FOLIAGE_PLACER),
        "minecraft:random_spread_foliage_placer" => {
            Some(FoliagePlacerTypes::RANDOM_SPREAD_FOLIAGE_PLACER)
        }
        "minecraft:cherry_foliage_placer" => Some(FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.FOLIAGE_PLACER_TYPE` element ids equal the
        // insertion index in `FoliagePlacerType.java`'s declaration order.
        assert_eq!(FoliagePlacerTypes::BLOB_FOLIAGE_PLACER.id, 0);
        assert_eq!(FoliagePlacerTypes::SPRUCE_FOLIAGE_PLACER.id, 1);
        assert_eq!(FoliagePlacerTypes::PINE_FOLIAGE_PLACER.id, 2);
        assert_eq!(FoliagePlacerTypes::ACACIA_FOLIAGE_PLACER.id, 3);
        assert_eq!(FoliagePlacerTypes::BUSH_FOLIAGE_PLACER.id, 4);
        assert_eq!(FoliagePlacerTypes::FANCY_FOLIAGE_PLACER.id, 5);
        assert_eq!(FoliagePlacerTypes::JUNGLE_FOLIAGE_PLACER.id, 6);
        assert_eq!(FoliagePlacerTypes::MEGA_PINE_FOLIAGE_PLACER.id, 7);
        assert_eq!(FoliagePlacerTypes::DARK_OAK_FOLIAGE_PLACER.id, 8);
        assert_eq!(FoliagePlacerTypes::RANDOM_SPREAD_FOLIAGE_PLACER.id, 9);
        assert_eq!(FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER.id, 10);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(
            FoliagePlacerTypes::BLOB_FOLIAGE_PLACER.location,
            "minecraft:blob_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::SPRUCE_FOLIAGE_PLACER.location,
            "minecraft:spruce_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::PINE_FOLIAGE_PLACER.location,
            "minecraft:pine_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::ACACIA_FOLIAGE_PLACER.location,
            "minecraft:acacia_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::BUSH_FOLIAGE_PLACER.location,
            "minecraft:bush_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::FANCY_FOLIAGE_PLACER.location,
            "minecraft:fancy_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::JUNGLE_FOLIAGE_PLACER.location,
            "minecraft:jungle_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::MEGA_PINE_FOLIAGE_PLACER.location,
            "minecraft:mega_pine_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::DARK_OAK_FOLIAGE_PLACER.location,
            "minecraft:dark_oak_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::RANDOM_SPREAD_FOLIAGE_PLACER.location,
            "minecraft:random_spread_foliage_placer"
        );
        assert_eq!(
            FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER.location,
            "minecraft:cherry_foliage_placer"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        for id in [
            FoliagePlacerTypes::BLOB_FOLIAGE_PLACER,
            FoliagePlacerTypes::SPRUCE_FOLIAGE_PLACER,
            FoliagePlacerTypes::PINE_FOLIAGE_PLACER,
            FoliagePlacerTypes::ACACIA_FOLIAGE_PLACER,
            FoliagePlacerTypes::BUSH_FOLIAGE_PLACER,
            FoliagePlacerTypes::FANCY_FOLIAGE_PLACER,
            FoliagePlacerTypes::JUNGLE_FOLIAGE_PLACER,
            FoliagePlacerTypes::MEGA_PINE_FOLIAGE_PLACER,
            FoliagePlacerTypes::DARK_OAK_FOLIAGE_PLACER,
            FoliagePlacerTypes::RANDOM_SPREAD_FOLIAGE_PLACER,
            FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER,
        ] {
            assert_eq!(foliage_placer_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(foliage_placer_type_by_name("minecraft:nope"), None);
        assert_eq!(foliage_placer_type_by_name("blob_foliage_placer"), None);
    }
}
