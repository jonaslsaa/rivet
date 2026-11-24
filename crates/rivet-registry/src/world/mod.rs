//! `net.minecraft.world` classes that live below `rivet-world` because
//! `net.minecraft.core` and `rivet-protocol` use them (D15; see the CLASS_CRATES
//! table in `scripts/coverage_ledger.py`): the `world.phys` value types and
//! `InteractionHand`.

pub mod interaction_hand;
pub mod phys;
