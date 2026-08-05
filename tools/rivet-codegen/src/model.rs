//! Data model for the block-state registry extracted from the Paper jar.
//!
//! This is the canonical intermediate artifact: `extract` produces it from the
//! real 26.2 jar, `generate` consumes it to emit Rust source for
//! `crates/rivet-registry`.

use serde::{Deserialize, Serialize};

/// Root of `data/block_states.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRegistry {
    /// MC version the data was extracted from, e.g. `"26.2"`.
    pub minecraft_version: String,
    /// All blocks in registry-id order (id is the numeric vanilla block id).
    pub blocks: Vec<BlockDef>,
}

/// A single block type (one entry in `minecraft:block`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDef {
    /// Numeric registry id (matches `BuiltInRegistries.BLOCK.getId`).
    pub id: u16,
    /// Namespaced key, e.g. `"minecraft:creaking_heart"`.
    pub name: String,
    /// State properties in `StateDefinition` declaration order.
    pub properties: Vec<BlockProperty>,
}

/// A state property. `values` preserve the Java order returned by
/// `Property.getPossibleValues()` — that ordering defines the block-state
/// index layout, so it must not be sorted/rewritten during codegen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockProperty {
    pub name: String,
    pub values: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_round_trips() {
        let data = BlockRegistry {
            minecraft_version: "26.2".into(),
            blocks: vec![BlockDef {
                id: 0,
                name: "minecraft:air".into(),
                properties: vec![BlockProperty {
                    name: "waterlogged".into(),
                    values: vec!["true".into(), "false".into()],
                }],
            }],
        };
        let json = serde_json::to_string(&data).unwrap();
        let back: BlockRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.minecraft_version, "26.2");
        assert_eq!(back.blocks[0].name, "minecraft:air");
        assert_eq!(back.blocks[0].properties[0].values, ["true", "false"]);
    }
}
