//! `net.minecraft.world.level.material.Fluids` — the pure fluid id-handle
//! (issue #370).
//!
//! Java's `Fluid` is a behaviour-carrying object registered in
//! `BuiltInRegistries.FLUID` (a `DefaultedRegistry` whose default element is
//! `minecraft:empty`). This slice carries only the numeric id-handle backed by
//! the generated `FLUID_BY_NAME`/`FLUID_BY_ID`/`FLUID_DEFAULT` tables
//! (codegen-owned, in `generated::registries`), mirroring the `BlockId`
//! ownership model: no fluid behaviour abstraction is introduced, and the
//! `SavedTick<FluidId>` codecs this slice feeds only need to identify a stored
//! payload.
//!
//! Out-of-range numeric ids degrade to the default fluid's name on `name()`,
//! matching `DefaultedRegistry.byId`'s fallback (like `BlockId`'s air
//! fallback).

use crate::generated::registries::{FLUID_BY_ID, FLUID_BY_NAME};

/// A numeric vanilla fluid id (index into the `minecraft:fluid` registry).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FluidId(pub u16);

impl FluidId {
    /// `Fluids.EMPTY` — the default fluid (`minecraft:empty`, id 0).
    pub const EMPTY: FluidId = FluidId(0);
    /// `Fluids.WATER` — `minecraft:water` (id 2).
    pub const WATER: FluidId = FluidId(2);
    /// `Fluids.LAVA` — `minecraft:lava` (id 4).
    pub const LAVA: FluidId = FluidId(4);

    #[inline]
    pub const fn from_id(id: u16) -> Self {
        Self(id)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        FLUID_BY_NAME.get(name).copied().map(Self)
    }

    /// The default fluid name (`minecraft:empty`), like `BlockId::name`'s air
    /// fallback.
    pub fn name(self) -> &'static str {
        FLUID_BY_ID
            .get(self.0 as usize)
            .copied()
            .unwrap_or(crate::generated::registries::FLUID_DEFAULT)
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
    fn fluid_names_and_ids_are_generated() {
        assert_eq!(FluidId::from_name("minecraft:empty").unwrap().id(), 0);
        assert_eq!(FluidId::from_name("minecraft:water").unwrap().id(), 2);
        assert_eq!(FluidId::from_name("minecraft:lava").unwrap().id(), 4);
        assert_eq!(
            FluidId::from_name("minecraft:flowing_water").unwrap().id(),
            1
        );
        assert_eq!(FluidId::from_id(0).name(), "minecraft:empty");
        assert_eq!(FluidId::from_id(2).name(), "minecraft:water");
        // Unknown names are not representable (DefaultedRegistry getValue falls
        // back, but the pure id-handle stays strict like BlockId::from_name).
        assert_eq!(FluidId::from_name("minecraft:not_a_fluid"), None);
        // Out-of-range ids degrade to the default fluid name.
        assert_eq!(FluidId::from_id(999).name(), "minecraft:empty");
        assert_eq!(FluidId::from_id(u16::MAX).name(), "minecraft:empty");
    }

    #[test]
    fn constants_match_generated_names_and_ids() {
        for fluid in [FluidId::EMPTY, FluidId::WATER, FluidId::LAVA] {
            let by_name = FluidId::from_name(fluid.name())
                .unwrap_or_else(|| panic!("generated name `{}` must resolve", fluid.name()));
            assert_eq!(fluid.id(), by_name.id());
            assert_eq!(fluid.name(), by_name.name());
        }
    }
}
