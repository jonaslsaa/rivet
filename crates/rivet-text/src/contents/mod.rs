//! Port of `net.minecraft.network.chat.contents` — the concrete
//! `ComponentContents` implementations and their MAP_CODECs.
//!
//! This slice ports the five content types that appear in login/configuration
//! disconnect and basic system messages: PlainText, Translatable, Keybind,
//! Score, and Selector. The `Nbt` and `Object` contents (and their `DataSource`
//! / `ObjectInfo` registries) are deferred: Nbt requires `NbtOps`/path parsing
//! and Object requires `FontDescription` identifiers — both are later epic #12
//! slices.

pub mod keybind;
pub mod plain_text;
pub mod score;
pub mod selector;
pub mod translatable;

pub use keybind::KeybindContents;
pub use plain_text::PlainTextContents;
pub use score::{ScoreContents, ScoreName};
pub use selector::SelectorContents;
pub use translatable::{TranslatableArg, TranslatableContents};
