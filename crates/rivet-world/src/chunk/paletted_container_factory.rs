//! Port of `net.minecraft.world.level.chunk.PalettedContainerFactory` (MC 26.2)
//! — the read-view closure over the #108/#216 wire core.
//!
//! Java: `PalettedContainerFactory.java` in `working/Paper`. The factory is a
//! record holding the two strategies (block states, biomes), the two codec
//! default values (air, plains), and the three container codecs that the
//! `SerializableChunkData` section reader uses to parse a section tag's
//! `block_states`/`biomes` compounds. `create(RegistryAccess)` pulls the
//! strategies and defaults from the block-state/biome registries.
//!
//! The Rust port keeps the four registry-derived fields and the two
//! `createFor*` producers; `create(RegistryAccess)` becomes [`new`] with the
//! caller's strategies/defaults (rivet-world has no `RegistryAccess` — the
//! superflat and server callers build the dense maps and hand them in). The
//! read-view decode is [`PalettedContainer::unpack`], which has the exact shape
//! of Java's `PalettedContainerRO.Unpacker` (`strategy, PackedData ->
//! Result<container, String>`), so the DFU `Codec`/`Unpacker` wiring is not
//! needed to parse a section tag.
//!
//! RivetTodo(#202): the three codec fields are not ported — each needs the
//! element codec for the palette entries (`BlockState.CODEC` for the block
//! container, `biomes.holderByNameCodec()` for the biome container), and
//! `BlockState.CODEC` is deferred with #202's `NbtUtils.readBlockState`
//! marker.
//! The Anti-Xray `createForBlockStates(Level, ChunkPos, int)`
//! overload (preset block states from the `ChunkPacketBlockController`) is
//! omitted — `Level`/`ChunkPacketBlockController` are not ported, deferred
//! with the `paper.antixray` chunk-storage unit.

use crate::chunk::paletted_container::PalettedContainer;
use crate::chunk::strategy::Strategy;

/// `net.minecraft.world.level.chunk.PalettedContainerFactory` — the four
/// registry-derived fields of the Java record. `T` is the block-state type
/// (`StateId` in the generated-table callers), `B` the biome type.
pub struct PalettedContainerFactory<T, B>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
{
    /// `Strategy<BlockState>` — `Strategy.createForBlockStates`.
    block_states_strategy: Strategy<T>,
    /// `BlockState defaultBlockState` — `Blocks.AIR.defaultBlockState()`.
    default_block_state: T,
    /// `Strategy<Holder<Biome>>` — `Strategy.createForBiomes`.
    biome_strategy: Strategy<B>,
    /// `Holder<Biome> defaultBiome` — the plains biome holder.
    default_biome: B,
}

impl<
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
> PalettedContainerFactory<T, B>
{
    /// `create(RegistryAccess)` — Java reads the four fields out of the
    /// registries; the port receives them directly (the caller's registry
    /// access produces the strategies and defaults).
    pub fn new(
        block_states_strategy: Strategy<T>,
        default_block_state: T,
        biome_strategy: Strategy<B>,
        default_biome: B,
    ) -> Self {
        PalettedContainerFactory {
            block_states_strategy,
            default_block_state,
            biome_strategy,
            default_biome,
        }
    }

    /// `createForBlockStates()` — `new PalettedContainer<>(defaultBlockState,
    /// blockStatesStrategy, null)` (no Anti-Xray preset values).
    pub fn create_for_block_states(&self) -> PalettedContainer<T> {
        PalettedContainer::new(
            self.default_block_state.clone(),
            self.block_states_strategy.clone(),
        )
    }

    /// `createForBiomes()` — `new PalettedContainer<>(defaultBiome,
    /// biomeStrategy, null)`.
    pub fn create_for_biomes(&self) -> PalettedContainer<B> {
        PalettedContainer::new(self.default_biome.clone(), self.biome_strategy.clone())
    }

    /// The record accessor `blockStatesStrategy()`.
    pub fn block_states_strategy(&self) -> &Strategy<T> {
        &self.block_states_strategy
    }

    /// The record accessor `defaultBlockState()`.
    pub fn default_block_state(&self) -> &T {
        &self.default_block_state
    }

    /// The record accessor `biomeStrategy()`.
    pub fn biome_strategy(&self) -> &Strategy<B> {
        &self.biome_strategy
    }

    /// The record accessor `defaultBiome()`.
    pub fn default_biome(&self) -> &B {
        &self.default_biome
    }
}
