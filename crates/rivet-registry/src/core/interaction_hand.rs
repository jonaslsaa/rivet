//! `net.minecraft.world.InteractionHand` — the two hands (#448).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/InteractionHand.java`. Ported as the value enum the serverbound
//! interaction packets carry (`ServerboundUseItemOnPacket` /
//! `ServerboundUseItemPacket` read it with `FriendlyByteBuf.readEnum`, i.e. by
//! ordinal).
//!
//! Placement follows the documented `Difficulty`/`GameType` precedent
//! (OWNERSHIP.md §Registries — pure value types stay in `rivet-registry::core`,
//! with only their wire codecs crossing to `rivet-protocol`): `rivet-world →
//! rivet-protocol` already exists, so a `rivet-protocol → rivet-world` edge
//! for the packets would be a cycle.
//!
//! Deferred to the `mc.world` unit (no declarations emitted): `asEquipmentSlot`
//! (needs `EquipmentSlot`) and the `ByIdMap`-backed `STREAM_CODEC` (no
//! in-scope packet uses it; the interaction packets use `readEnum`).

/// `InteractionHand` — `MAIN_HAND(0)`, `OFF_HAND(1)` in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InteractionHand {
    /// `MAIN_HAND` — ordinal and id `0`.
    MainHand,
    /// `OFF_HAND` — ordinal and id `1`.
    OffHand,
}

impl InteractionHand {
    /// `InteractionHand.values()` — declaration (ordinal) order.
    pub const VALUES: [InteractionHand; 2] = [InteractionHand::MainHand, InteractionHand::OffHand];

    /// `ordinal()`.
    pub fn ordinal(self) -> i32 {
        match self {
            InteractionHand::MainHand => 0,
            InteractionHand::OffHand => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinals_follow_declaration_order() {
        for (index, hand) in InteractionHand::VALUES.iter().enumerate() {
            assert_eq!(hand.ordinal(), index as i32);
        }
    }
}
