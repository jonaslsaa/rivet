//! `net.minecraft.world.InteractionHand`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/InteractionHand.java`.
//!
//! Home note (D15, `scripts/coverage_ledger.py` CLASS_CRATES): `rivet-protocol`'s
//! `ServerboundUseItemPacket` and `ServerboundUseItemOnPacket` carry an
//! `InteractionHand`, and `rivet-world` depends on `rivet-protocol`, so the one
//! definition cannot live above `rivet-registry`. `asEquipmentSlot()` returns
//! `EquipmentSlot` (a live-game type), so it is
//! `InteractionHandLive::as_equipment_slot` in `rivet-server` at the same
//! mirrored path. `STREAM_CODEC` lives in `rivet-protocol`.

use rivet_util::by_id_map::{self, OutOfBoundsStrategy};

/// `InteractionHand` — `MAIN_HAND(0)`, `OFF_HAND(1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InteractionHand {
    MainHand,
    OffHand,
}

impl InteractionHand {
    /// `InteractionHand.values()`.
    pub const VALUES: [InteractionHand; 2] = [InteractionHand::MainHand, InteractionHand::OffHand];

    /// `InteractionHand.id`.
    pub const fn id(self) -> i32 {
        match self {
            InteractionHand::MainHand => 0,
            InteractionHand::OffHand => 1,
        }
    }

    /// `Enum.ordinal()` — declaration order, equal to `id`.
    pub const fn ordinal(self) -> i32 {
        self.id()
    }

    /// `InteractionHand.BY_ID.apply(id)` — `ByIdMap.continuous(h -> h.id,
    /// values(), OutOfBoundsStrategy.ZERO)`: out-of-range ids map to
    /// `MAIN_HAND`.
    pub fn by_id(id: i32) -> InteractionHand {
        by_id_map::continuous(
            |hand: &InteractionHand| hand.id(),
            &InteractionHand::VALUES,
            OutOfBoundsStrategy::Zero,
        )(id)
    }
}
