//! The pure biome id-handle over the generated `minecraft:worldgen/biome`
//! tables (issue #180).
//!
//! Java's `Biome` is a behaviour-carrying object registered in
//! `BuiltInRegistries.BIOME`. This slice carries only the numeric id-handle
//! backed by the generated `BIOME_BY_NAME`/`BIOME_BY_ID`/`BIOME_COUNT` tables
//! (codegen-owned, in `generated::biomes`), mirroring the `BlockId`/`FluidId`
//! ownership model: the `MatchingBiomesPredicate` codec this slice feeds only
//! needs to identify a `Holder<Biome>` reference (its `(RegistryId, u32)` pair
//! — equality never dereferences the element value), and `HolderSet.contains`
//! compares those Copy pairs. The `Biome` value type itself defers with the
//! `mc.world.level.biome` unit.
//!
//! Out-of-range numeric ids degrade to the default biome name on `name()`
//! (biome id 0 is `minecraft:badlands`, alphabetical insertion — there is no
//! DefaultedRegistry-style "default element", so the fallback is the first
//! table entry, mirroring `DefaultedRegistry.byId`'s out-of-range behaviour of
//! returning null, which the holder layer treats as absent).

use crate::generated::biomes::{BIOME_BY_ID, BIOME_BY_NAME};

/// A numeric vanilla biome id (index into the `minecraft:worldgen/biome` registry).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BiomeId(pub u16);

impl BiomeId {
    #[inline]
    pub const fn from_id(id: u16) -> Self {
        Self(id)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        BIOME_BY_NAME.get(name).copied().map(Self)
    }

    /// The biome name (`minecraft:badlands` for id 0). An out-of-range id
    /// resolves to the first table entry (id 0).
    pub fn name(self) -> &'static str {
        BIOME_BY_ID
            .get(self.0 as usize)
            .copied()
            .unwrap_or(BIOME_BY_ID[0])
    }

    #[inline]
    pub const fn id(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_names_and_ids_are_generated() {
        assert_eq!(BiomeId::from_name("minecraft:plains").unwrap().id(), 40);
        assert_eq!(BiomeId::from_name("minecraft:the_void").unwrap().id(), 58);
        assert_eq!(BiomeId::from_id(0).name(), "minecraft:badlands");
        assert_eq!(BiomeId::from_id(40).name(), "minecraft:plains");
        // Unknown names are not representable.
        assert_eq!(BiomeId::from_name("minecraft:not_a_biome"), None);
        // Out-of-range ids degrade to the first table entry.
        assert_eq!(BiomeId::from_id(999).name(), "minecraft:badlands");
        assert_eq!(BiomeId::from_id(u16::MAX).name(), "minecraft:badlands");
    }

    #[test]
    fn every_biome_name_round_trips() {
        for (id, name) in crate::generated::biomes::BIOME_BY_ID.iter().enumerate() {
            let by_name = BiomeId::from_name(name).unwrap_or_else(|| panic!("{name} must resolve"));
            assert_eq!(by_name.id(), id as u16);
            assert_eq!(by_name.name(), *name);
        }
    }
}
