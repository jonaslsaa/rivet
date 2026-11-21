//! `net.minecraft.world.item.trading.Merchant`.

use super::merchant_offers::MerchantOffers;

/// Mirrors `net.minecraft.world.item.trading.Merchant`.
pub trait Merchant {
    fn offers(&self) -> &MerchantOffers;
    fn offers_mut(&mut self) -> &mut MerchantOffers;
    fn notify_trade(&mut self, offer_index: usize);
    fn villager_xp(&self) -> i32;
    fn show_progress_bar(&self) -> bool;

    /// Mirrors `Merchant.canRestock()` (default `false`; `Villager` overrides to `true`).
    fn can_restock(&self) -> bool {
        false
    }
}
