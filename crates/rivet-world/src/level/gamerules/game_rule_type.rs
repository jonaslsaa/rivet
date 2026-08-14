//! `net.minecraft.world.level.gamerules.GameRuleType` — the enum classifying a
//! game rule's value type.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! gamerules/GameRuleType.java`.

use rivet_util::string_representable::StringRepresentable;
use std::fmt;

/// `GameRuleType` — `INT("integer")`, `BOOL("boolean")`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameRuleType {
    Int,
    Bool,
}

impl StringRepresentable for GameRuleType {
    /// `getSerializedName()` — `"integer"` / `"boolean"`.
    fn get_serialized_name(&self) -> &str {
        match self {
            GameRuleType::Int => "integer",
            GameRuleType::Bool => "boolean",
        }
    }
}

impl fmt::Display for GameRuleType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.get_serialized_name())
    }
}
