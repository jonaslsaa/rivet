//! `net.minecraft.world.level.block` — the block handle (issue #228).
//!
//! Java's `Block` is a behaviour-carrying object registered in
//! `BuiltInRegistries.BLOCK`; the generated tables make every behaviour this
//! slice needs derivable from the registry id, so the id-handle [`Block`] (a
//! `BlockId` newtype) is the full value type here (OWNERSHIP: arenas + ids).
//! [`blocks`] holds the `Blocks` constant table — the named subset of blocks
//! worldgen/lighting/`NbtUtils.readBlockState` reference.
//!
//! The value surface (`BlockState`, `StateDefinition`, `Property`, `MapColor`)
//! lives in `rivet-registry` (issue #228), where it decodes the generated
//! tables without a world dependency; this module only adds the `Block` handle
//! and the named constants on top.

pub mod blocks;
/// `net.minecraft.world.level.block.state` — the package mirror hosting the
/// `state.predicate` sub-package (issue #228); the `BlockState`/`StateDefinition`
/// value types themselves live in `rivet-registry` (issue #228).
pub mod state;

use rivet_registry::block_state::BlockState;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::state_definition::StateDefinition;

/// `net.minecraft.world.level.block.Block` — the id-handle of a registered
/// block. `Copy`/`Eq` mirror the wrapped id, like `BlockState`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Block(BlockId);

impl Block {
    /// Wrap a registry id. Ids past the block table are valid handles and
    /// degrade to air on the operations that consult the tables (mirroring
    /// `Block.stateById`, which falls back to `Blocks.AIR.defaultBlockState()`).
    #[inline]
    pub const fn new(id: BlockId) -> Self {
        Self(id)
    }

    /// The registry id.
    #[inline]
    pub const fn id(self) -> BlockId {
        self.0
    }

    /// The minimal by-name accessor (#370): resolve a namespaced registry id
    /// (e.g. `"minecraft:stone"`) to the id-handle. Unknown names are `None`,
    /// matching `BlockId::from_name` (no defaulted fallback).
    #[inline]
    pub fn from_name(name: &str) -> Option<Self> {
        BlockId::from_name(name).map(Self)
    }

    /// The registry key (`BuiltInRegistries.BLOCK.getKey(block).toString()`,
    /// e.g. `"minecraft:stone"`).
    #[inline]
    pub fn name(self) -> &'static str {
        self.0.name()
    }

    /// `block.defaultBlockState()` — the block's default state.
    #[inline]
    pub fn default_block_state(self) -> BlockState {
        BlockState::of(self.0)
    }

    /// `block.getStateDefinition()` — the block's property definition.
    #[inline]
    pub fn state_definition(self) -> StateDefinition {
        StateDefinition::for_block(self.0)
    }
}
