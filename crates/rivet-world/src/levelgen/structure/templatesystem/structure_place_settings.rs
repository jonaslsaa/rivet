//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.StructurePlaceSettings`
//! (MC 26.2, including Paper/CraftBukkit `palette = -1` bounds check).

use super::liquid_settings::LiquidSettings;
use super::structure_processor::ErasedStructureProcessor;
use super::structure_template::{Mirror, Palette};
use crate::levelgen::structure::bounding_box::BoundingBox;
use rivet_registry::core::{BlockPos, Rotation};
use rivet_util::RandomSource;
use rivet_util::mth;
use rivet_util::random::LegacyRandomSource;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// `net.minecraft.world.level.levelgen.structure.templatesystem.StructurePlaceSettings`.
#[derive(Clone, Debug)]
pub struct StructurePlaceSettings {
    mirror: Mirror,
    rotation: Rotation,
    rotation_pivot: BlockPos,
    ignore_entities: bool,
    bounding_box: Option<BoundingBox>,
    liquid_settings: LiquidSettings,
    /// Optional explicit seed for deterministic random draws (mirrors Java's
    /// `@Nullable RandomSource random` when set via `setRandom`).
    random_seed: Option<i64>,
    /// Paper/CraftBukkit: `-1` means choose a random palette; `>= 0` selects
    /// that exact palette index and errors if out of bounds.
    pub palette: i32,
    processors: Vec<Arc<dyn ErasedStructureProcessor>>,
    known_shape: bool,
    finalize_entities: bool,
}

impl Default for StructurePlaceSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl StructurePlaceSettings {
    pub fn new() -> Self {
        Self {
            mirror: Mirror::None,
            rotation: Rotation::None,
            rotation_pivot: BlockPos::ZERO,
            ignore_entities: false,
            bounding_box: None,
            liquid_settings: LiquidSettings::ApplyWaterlogging,
            random_seed: None,
            palette: -1,
            processors: Vec::new(),
            known_shape: false,
            finalize_entities: false,
        }
    }

    pub fn copy(&self) -> Self {
        self.clone()
    }

    pub fn set_mirror(&mut self, mirror: Mirror) -> &mut Self {
        self.mirror = mirror;
        self
    }

    pub fn set_rotation(&mut self, rotation: Rotation) -> &mut Self {
        self.rotation = rotation;
        self
    }

    pub fn set_rotation_pivot(&mut self, pivot: BlockPos) -> &mut Self {
        self.rotation_pivot = pivot;
        self
    }

    pub fn set_ignore_entities(&mut self, ignore_entities: bool) -> &mut Self {
        self.ignore_entities = ignore_entities;
        self
    }

    pub fn set_bounding_box(&mut self, bounding_box: Option<BoundingBox>) -> &mut Self {
        self.bounding_box = bounding_box;
        self
    }

    pub fn set_random_seed(&mut self, seed: Option<i64>) -> &mut Self {
        self.random_seed = seed;
        self
    }

    pub fn set_liquid_settings(&mut self, liquid_settings: LiquidSettings) -> &mut Self {
        self.liquid_settings = liquid_settings;
        self
    }

    pub fn set_known_shape(&mut self, known_shape: bool) -> &mut Self {
        self.known_shape = known_shape;
        self
    }

    pub fn set_finalize_entities(&mut self, finalize_entities: bool) -> &mut Self {
        self.finalize_entities = finalize_entities;
        self
    }

    pub fn clear_processors(&mut self) -> &mut Self {
        self.processors.clear();
        self
    }

    pub fn add_processor(&mut self, processor: Arc<dyn ErasedStructureProcessor>) -> &mut Self {
        self.processors.push(processor);
        self
    }

    /// `StructurePlaceSettings.popProcessor(StructureProcessor)` — removes the
    /// first processor matching by `Arc::ptr_eq`.
    pub fn pop_processor(&mut self, processor: &Arc<dyn ErasedStructureProcessor>) -> &mut Self {
        if let Some(idx) = self
            .processors
            .iter()
            .position(|p| Arc::ptr_eq(p, processor))
        {
            self.processors.remove(idx);
        }
        self
    }

    pub fn get_mirror(&self) -> Mirror {
        self.mirror
    }

    pub fn get_rotation(&self) -> Rotation {
        self.rotation
    }

    pub fn get_rotation_pivot(&self) -> BlockPos {
        self.rotation_pivot
    }

    /// `StructurePlaceSettings.getRandom(@Nullable BlockPos)`.
    ///
    /// - If an explicit seed was configured, seeds `LegacyRandomSource` from it.
    /// - Otherwise, if `pos` is `Some(p)`, uses `Mth.getSeed(p)`.
    /// - Otherwise, uses current epoch millis (`Util.getMillis()`).
    pub fn get_random(&self, pos: Option<BlockPos>) -> LegacyRandomSource {
        if let Some(seed) = self.random_seed {
            return LegacyRandomSource::new(seed);
        }
        match pos {
            Some(p) => LegacyRandomSource::new(mth::get_seed(p.get_x(), p.get_y(), p.get_z())),
            None => {
                let millis = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                LegacyRandomSource::new(millis)
            }
        }
    }

    pub fn is_ignore_entities(&self) -> bool {
        self.ignore_entities
    }

    pub fn get_bounding_box(&self) -> Option<BoundingBox> {
        self.bounding_box
    }

    pub fn get_known_shape(&self) -> bool {
        self.known_shape
    }

    pub fn should_finalize_entities(&self) -> bool {
        self.finalize_entities
    }

    pub fn get_processors(&self) -> &[Arc<dyn ErasedStructureProcessor>] {
        &self.processors
    }

    pub fn get_liquid_settings(&self) -> LiquidSettings {
        self.liquid_settings
    }

    pub fn should_apply_waterlogging(&self) -> bool {
        self.liquid_settings == LiquidSettings::ApplyWaterlogging
    }

    /// `StructurePlaceSettings.getRandomPalette(List<Palette>, @Nullable BlockPos)`.
    ///
    /// Matches Paper/CraftBukkit behavior:
    /// - Empty `palettes` panics with `"No palettes"`.
    /// - `self.palette >= 0` selects that exact index or panics if out of bounds.
    /// - `self.palette < 0` picks `get_random(pos).next_int_bound(palettes.len())`.
    pub fn get_random_palette<'a>(
        &self,
        palettes: &'a [Palette],
        pos: Option<BlockPos>,
    ) -> &'a Palette {
        let palette_size = palettes.len();
        if palette_size == 0 {
            panic!("No palettes");
        }
        if self.palette >= 0 {
            let idx = self.palette as usize;
            if idx >= palette_size {
                panic!(
                    "Palette index out of bounds. Got {} where there are only {} palettes available.",
                    self.palette, palette_size
                );
            }
            &palettes[idx]
        } else {
            let mut rng = self.get_random(pos);
            let idx = rng.next_int_bound(palette_size as i32) as usize;
            &palettes[idx]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::structure_template::StructureBlockInfo;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;

    fn block_id(name: &str) -> BlockId {
        BlockId::from_name(name).unwrap_or_else(|| panic!("unknown block {name}"))
    }

    fn sample_palettes() -> Vec<Palette> {
        vec![
            Palette::new(vec![StructureBlockInfo::new(
                BlockPos::ZERO,
                BlockState::of(block_id("minecraft:stone")),
                None,
            )]),
            Palette::new(vec![StructureBlockInfo::new(
                BlockPos::ZERO,
                BlockState::of(block_id("minecraft:granite")),
                None,
            )]),
        ]
    }

    #[test]
    fn explicit_palette_index_selects_exact_palette() {
        let palettes = sample_palettes();
        let mut settings = StructurePlaceSettings::new();
        settings.palette = 1;
        let selected = settings.get_random_palette(&palettes, Some(BlockPos::ZERO));
        assert_eq!(
            selected.blocks()[0].state.block(),
            block_id("minecraft:granite")
        );
    }

    #[test]
    #[should_panic(expected = "No palettes")]
    fn empty_palettes_panics() {
        let settings = StructurePlaceSettings::new();
        let _ = settings.get_random_palette(&[], Some(BlockPos::ZERO));
    }

    #[test]
    #[should_panic(expected = "Palette index out of bounds")]
    fn out_of_bounds_palette_index_panics() {
        let palettes = sample_palettes();
        let mut settings = StructurePlaceSettings::new();
        settings.palette = 2;
        let _ = settings.get_random_palette(&palettes, Some(BlockPos::ZERO));
    }

    #[test]
    fn default_random_palette_is_deterministic_for_position() {
        let palettes = sample_palettes();
        let settings = StructurePlaceSettings::new();
        let pos = BlockPos::new(12, 64, -30);
        let a = settings.get_random_palette(&palettes, Some(pos));
        let b = settings.get_random_palette(&palettes, Some(pos));
        assert_eq!(a, b);
    }
}
