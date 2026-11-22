//! Wandering trader merchant state mirroring Paper 26.2 `WanderingTrader`.

use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};

use super::data_component_exact_predicate::TradingCodecError;
use super::merchant::Merchant;
use super::merchant_offers::MerchantOffers;

/// Wandering trader merchant state mirroring Paper 26.2 `WanderingTrader`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WanderingTraderMerchantState {
    pub offers: MerchantOffers,
    pub despawn_delay: i32,
}

impl WanderingTraderMerchantState {
    pub fn new(offers: MerchantOffers, despawn_delay: i32) -> Self {
        Self {
            offers,
            despawn_delay,
        }
    }

    /// Saves wandering trader fields (`Offers`, `DespawnDelay`).
    pub fn save_to_nbt(&self, tag: &mut CompoundTag) {
        if !self.offers.is_empty() {
            tag.put("Offers".to_string(), Tag::Compound(self.offers.to_nbt()));
        }
        tag.put_int("DespawnDelay", self.despawn_delay);
    }

    /// Loads wandering trader fields from NBT.
    pub fn load_from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
        let offers = match tag.get_compound("Offers") {
            Some(offers_tag) => MerchantOffers::from_nbt(offers_tag)?,
            None => MerchantOffers::new(),
        };
        Ok(Self {
            offers,
            despawn_delay: tag.get_int("DespawnDelay").unwrap_or(0),
        })
    }
}

impl Merchant for WanderingTraderMerchantState {
    fn offers(&self) -> &MerchantOffers {
        &self.offers
    }

    fn offers_mut(&mut self) -> &mut MerchantOffers {
        &mut self.offers
    }

    fn notify_trade(&mut self, offer_index: usize) {
        if let Some(offer) = self.offers.get_mut(offer_index) {
            offer.increase_uses();
        }
    }

    fn villager_xp(&self) -> i32 {
        0
    }

    /// Mirrors `WanderingTrader.showProgressBar()` -> `false`.
    fn show_progress_bar(&self) -> bool {
        false
    }
}
