//! Player recipe-book state and settings.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.stats.RecipeBook`
//! - `net.minecraft.stats.RecipeBookSettings`
//! - `net.minecraft.stats.ServerRecipeBook`

use indexmap::IndexSet;

use super::display::{RecipeDisplayEntry, RecipeDisplayId};
use super::manager::RecipeManager;
use super::recipe_type::RecipeBookType;
use super::recipes::{RecipeHolder, RecipeKey};

/// `net.minecraft.stats.RecipeBookSettings.TypeSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TypeSettings {
    pub open: bool,
    pub filtering: bool,
}

impl TypeSettings {
    pub const DEFAULT: Self = Self {
        open: false,
        filtering: false,
    };

    pub const fn set_open(self, open: bool) -> Self {
        Self {
            open,
            filtering: self.filtering,
        }
    }

    pub const fn set_filtering(self, filtering: bool) -> Self {
        Self {
            open: self.open,
            filtering,
        }
    }
}

/// `net.minecraft.stats.RecipeBookSettings` — four mutable settings groups
/// (`crafting`, `furnace`, `blast_furnace`, `smoker`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RecipeBookSettings {
    pub crafting: TypeSettings,
    pub furnace: TypeSettings,
    pub blast_furnace: TypeSettings,
    pub smoker: TypeSettings,
}

impl RecipeBookSettings {
    pub const fn get(&self, book_type: RecipeBookType) -> TypeSettings {
        match book_type {
            RecipeBookType::Crafting => self.crafting,
            RecipeBookType::Furnace => self.furnace,
            RecipeBookType::BlastFurnace => self.blast_furnace,
            RecipeBookType::Smoker => self.smoker,
        }
    }

    pub fn set(&mut self, book_type: RecipeBookType, settings: TypeSettings) {
        match book_type {
            RecipeBookType::Crafting => self.crafting = settings,
            RecipeBookType::Furnace => self.furnace = settings,
            RecipeBookType::BlastFurnace => self.blast_furnace = settings,
            RecipeBookType::Smoker => self.smoker = settings,
        }
    }

    pub const fn is_open(&self, book_type: RecipeBookType) -> bool {
        self.get(book_type).open
    }

    pub fn set_open(&mut self, book_type: RecipeBookType, open: bool) {
        let updated = self.get(book_type).set_open(open);
        self.set(book_type, updated);
    }

    pub const fn is_filtering(&self, book_type: RecipeBookType) -> bool {
        self.get(book_type).filtering
    }

    pub fn set_filtering(&mut self, book_type: RecipeBookType, filtering: bool) {
        let updated = self.get(book_type).set_filtering(filtering);
        self.set(book_type, updated);
    }

    pub fn replace_from(&mut self, other: &Self) {
        *self = *other;
    }
}

/// Entry emitted for `ClientboundRecipeBookAddPacket` (`ServerRecipeBook.addRecipes` /
/// `sendInitialRecipeBook`).
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeBookAddEntry {
    pub contents: RecipeDisplayEntry,
    pub notification: bool,
    pub highlight: bool,
}

/// `net.minecraft.stats.ServerRecipeBook.Packed` — serializable snapshot of a
/// player's recipe-book state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedServerRecipeBook {
    pub settings: RecipeBookSettings,
    pub known: Vec<RecipeKey>,
    pub highlight: Vec<RecipeKey>,
}

/// `net.minecraft.stats.ServerRecipeBook` (extending `RecipeBook`).
///
/// Uses `IndexSet` to preserve deterministic insertion order for `known` and
/// `highlight` sets (matching Paper's observable display resolution order).
#[derive(Debug, Clone, Default)]
pub struct ServerRecipeBook {
    book_settings: RecipeBookSettings,
    known: IndexSet<RecipeKey>,
    highlight: IndexSet<RecipeKey>,
}

impl ServerRecipeBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn book_settings(&self) -> &RecipeBookSettings {
        &self.book_settings
    }

    pub fn set_book_settings(&mut self, settings: RecipeBookSettings) {
        self.book_settings.replace_from(&settings);
    }

    pub const fn is_open(&self, book_type: RecipeBookType) -> bool {
        self.book_settings.is_open(book_type)
    }

    pub fn set_open(&mut self, book_type: RecipeBookType, open: bool) {
        self.book_settings.set_open(book_type, open);
    }

    pub const fn is_filtering(&self, book_type: RecipeBookType) -> bool {
        self.book_settings.is_filtering(book_type)
    }

    pub fn set_filtering(&mut self, book_type: RecipeBookType, filtering: bool) {
        self.book_settings.set_filtering(book_type, filtering);
    }

    pub fn set_book_setting(&mut self, book_type: RecipeBookType, open: bool, filtering: bool) {
        self.book_settings
            .set(book_type, TypeSettings { open, filtering });
    }

    /// `ServerRecipeBook.add(ResourceKey<Recipe<?>>)`.
    pub fn add(&mut self, id: RecipeKey) {
        self.known.insert(id);
    }

    /// `ServerRecipeBook.contains(ResourceKey<Recipe<?>>)`.
    pub fn contains(&self, id: &RecipeKey) -> bool {
        self.known.contains(id)
    }

    /// `ServerRecipeBook.remove(ResourceKey<Recipe<?>>)` — removes from both
    /// `known` and `highlight`.
    pub fn remove(&mut self, id: &RecipeKey) {
        self.known.shift_remove(id);
        self.highlight.shift_remove(id);
    }

    /// `ServerRecipeBook.removeHighlight(ResourceKey<Recipe<?>>)`.
    pub fn remove_highlight(&mut self, id: &RecipeKey) {
        self.highlight.shift_remove(id);
    }

    /// `ServerRecipeBook.addHighlight(ResourceKey<Recipe<?>>)`.
    pub fn add_highlight(&mut self, id: RecipeKey) {
        self.highlight.insert(id);
    }

    pub fn is_highlighted(&self, id: &RecipeKey) -> bool {
        self.highlight.contains(id)
    }

    pub fn known(&self) -> &IndexSet<RecipeKey> {
        &self.known
    }

    pub fn highlighted(&self) -> &IndexSet<RecipeKey> {
        &self.highlight
    }

    /// `ServerRecipeBook.addRecipes(Collection<RecipeHolder<?>>, ServerPlayer)`.
    ///
    /// Ignores special recipes and already-known recipes. Returns the newly
    /// awarded count and the list of `RecipeBookAddEntry` items to send in
    /// `ClientboundRecipeBookAddPacket`.
    pub fn add_recipes<'a, I>(
        &mut self,
        recipes: I,
        manager: &RecipeManager,
    ) -> (usize, Vec<RecipeBookAddEntry>)
    where
        I: IntoIterator<Item = &'a RecipeHolder>,
    {
        let mut added_count = 0usize;
        let mut entries = Vec::new();

        for holder in recipes {
            if !self.known.contains(&holder.id) && !holder.value.is_special() {
                self.add(holder.id.clone());
                self.add_highlight(holder.id.clone());
                added_count += 1;
                let show_notification = holder.value.show_notification();
                for display_entry in manager.list_displays_for_recipe(&holder.id) {
                    entries.push(RecipeBookAddEntry {
                        contents: display_entry.clone(),
                        notification: show_notification,
                        highlight: true,
                    });
                }
            }
        }

        (added_count, entries)
    }

    /// `ServerRecipeBook.removeRecipes(Collection<RecipeHolder<?>>, ServerPlayer)`.
    ///
    /// Returns the count of removed recipes and the `RecipeDisplayId`s to send
    /// in `ClientboundRecipeBookRemovePacket`.
    pub fn remove_recipes<'a, I>(
        &mut self,
        recipes: I,
        manager: &RecipeManager,
    ) -> (usize, Vec<RecipeDisplayId>)
    where
        I: IntoIterator<Item = &'a RecipeHolder>,
    {
        let mut removed_count = 0usize;
        let mut removed_displays = Vec::new();

        for holder in recipes {
            if self.known.contains(&holder.id) {
                self.remove(&holder.id);
                removed_count += 1;
                for display_entry in manager.list_displays_for_recipe(&holder.id) {
                    removed_displays.push(display_entry.id);
                }
            }
        }

        (removed_count, removed_displays)
    }

    /// `ServerRecipeBook.sendInitialRecipeBook(ServerPlayer)` — resolves all
    /// known recipes in deterministic `known` iteration order, marking
    /// highlighted ones with `highlight = true` and `notification = false`.
    pub fn initial_recipe_book_entries(&self, manager: &RecipeManager) -> Vec<RecipeBookAddEntry> {
        let mut entries = Vec::new();
        for key in &self.known {
            let is_highlighted = self.highlight.contains(key);
            for display_entry in manager.list_displays_for_recipe(key) {
                entries.push(RecipeBookAddEntry {
                    contents: display_entry.clone(),
                    notification: false,
                    highlight: is_highlighted,
                });
            }
        }
        entries
    }

    /// `ServerRecipeBook.copyOverData(ServerRecipeBook)`.
    pub fn copy_over_data(&mut self, other: &Self) {
        self.known = other.known.clone();
        self.highlight = other.highlight.clone();
        self.book_settings.replace_from(&other.book_settings);
    }

    /// `ServerRecipeBook.pack()`.
    pub fn pack(&self) -> PackedServerRecipeBook {
        PackedServerRecipeBook {
            settings: self.book_settings,
            known: self.known.iter().cloned().collect(),
            highlight: self.highlight.iter().cloned().collect(),
        }
    }

    /// `ServerRecipeBook.loadUntrusted(Packed, Predicate<ResourceKey<Recipe<?>>>)`.
    ///
    /// Replaces settings and filters untrusted `known` / `highlight` recipe IDs
    /// through `validator` (dropping unknown/removed recipes).
    pub fn load_untrusted<F>(&mut self, packed: PackedServerRecipeBook, mut validator: F)
    where
        F: FnMut(&RecipeKey) -> bool,
    {
        self.book_settings.replace_from(&packed.settings);
        self.known.clear();
        self.highlight.clear();

        for key in packed.known {
            if validator(&key) {
                self.add(key);
            }
        }
        for key in packed.highlight {
            if validator(&key) {
                self.add_highlight(key);
            }
        }
    }
}
