//! `ca.spottedleaf.moonrise.patches.starlight.light.StarLightEngine` — the
//! Starlight compute engine surface that the light tasks consume.
//!
//! Java: `StarLightEngine.java` in `working/Paper`. The chunk-light tasks
//! (`ChunkLightTask`, `ca...scheduling.task`) build the per-section emptiness
//! mask with the pure static `StarLightEngine.getEmptySectionsForChunk` and
//! pass it to the [`StarLightProvider`] ops; that static is ported here. The
//! propagation engines themselves (the sky/block `StarLightEngine`s and their
//! graph/nibble work) live in `rivet-server` (RivetTodo #184): the sky engine
//! is ported (`star_light_engine::SkyStarLightEngine`) and driven by the
//! `SkyLightProvider`; the block engine and the light queue defer.
//!
//! [`StarLightProvider`]: crate::lighting::star_light_provider::StarLightProvider

use crate::chunk::proto_chunk::ProtoChunk;

/// `StarLightEngine.getEmptySectionsForChunk(ChunkAccess)` — the per-section
/// emptiness mask the light tasks pass to
/// [`StarLightProvider::light_chunk`](crate::lighting::star_light_provider::StarLightProvider::light_chunk)
/// / `force_load_in_chunk`. Java fills `Boolean.TRUE` for a section that is
/// null or air-only and `Boolean.FALSE` otherwise; the port's chunk sections
/// are never null (the `Vec` is always full), so `has_only_air` decides. The
/// `Option<bool>` mirrors Java's tri-state `Boolean[]` (see the provider
/// trait doc): `Some(true)` empty, `Some(false)` has blocks.
pub fn get_empty_sections_for_chunk<T, B, S>(chunk: &ProtoChunk<T, B, S>) -> Vec<Option<bool>>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    chunk
        .get_sections()
        .iter()
        .map(|section| Some(section.has_only_air()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container::PalettedContainer;
    use crate::chunk::paletted_container_factory::PalettedContainerFactory;
    use crate::chunk::strategy::Strategy;
    use crate::chunk::upgrade_data::UpgradeData;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::heightmap::StateFlags;
    use rivet_registry::core::ChunkPos;

    #[derive(Clone, Copy)]
    struct TestGlobalMap;
    impl GlobalIdMap<u8> for TestGlobalMap {
        fn get_id(&self, value: &u8) -> i32 {
            *value as i32
        }
        fn by_id_or_throw(&self, id: i32) -> u8 {
            id as u8
        }
        fn size(&self) -> i32 {
            256
        }
        fn by_id(&self, id: i32) -> Option<u8> {
            Some(id as u8)
        }
        fn clone_box(&self) -> Box<dyn GlobalIdMap<u8> + Send + Sync> {
            Box::new(*self)
        }
    }

    fn is_air(s: &u8) -> bool {
        *s == 0
    }
    fn is_randomly_ticking(_s: &u8) -> bool {
        false
    }
    fn fluid_is_empty(_s: &u8) -> bool {
        true
    }
    fn fluid_is_randomly_ticking(_s: &u8) -> bool {
        false
    }
    fn is_special_colliding(_s: &u8) -> bool {
        false
    }

    fn section(stone_block: bool) -> crate::chunk::level_chunk_section::LevelChunkSection<u8, u8> {
        let mut states = PalettedContainer::new(
            0u8,
            Strategy::create_for_block_states(Box::new(TestGlobalMap)),
        );
        if stone_block {
            states.set(0, 0, 0, 1u8);
        }
        crate::chunk::level_chunk_section::LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, Strategy::create_for_biomes(Box::new(TestGlobalMap))),
            is_air,
            is_randomly_ticking,
            fluid_is_empty,
            fluid_is_randomly_ticking,
            is_special_colliding,
        )
    }

    /// `StarLightEngine.getEmptySectionsForChunk` is a per-section pure
    /// function of the chunk's sections: `Some(true)` for air-only, `Some(false)`
    /// for a section that has blocks.
    #[test]
    fn empty_sections_follow_section_emptiness() {
        let factory = PalettedContainerFactory::new(
            Strategy::create_for_block_states(Box::new(TestGlobalMap)),
            0,
            Strategy::create_for_biomes(Box::new(TestGlobalMap)),
            0,
        );
        // A three-section accessor (minY 0, height 48) so the section vec length
        // matches the accessor's section count.
        let sections = vec![section(true), section(false), section(true)];
        let chunk: ProtoChunk<u8, u8, &'static str> = ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(3),
            create_accessor(0, 48),
            &factory,
            Some(sections),
            0,
            255,
            &|s: &u8| StateFlags {
                is_air: *s == 0,
                blocks_motion: *s != 0,
                has_fluid: false,
                is_leaves: false,
            },
        );
        assert_eq!(
            get_empty_sections_for_chunk(&chunk),
            vec![Some(false), Some(true), Some(false)]
        );
    }

    /// An all-air chunk (the `sections: None` constructor default) is empty in
    /// every section.
    #[test]
    fn all_air_chunk_is_empty_everywhere() {
        let factory = PalettedContainerFactory::new(
            Strategy::create_for_block_states(Box::new(TestGlobalMap)),
            0,
            Strategy::create_for_biomes(Box::new(TestGlobalMap)),
            0,
        );
        let chunk: ProtoChunk<u8, u8, &'static str> = ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            create_accessor(-64, 384),
            &factory,
            None,
            0,
            255,
            &|s: &u8| StateFlags {
                is_air: *s == 0,
                blocks_motion: *s != 0,
                has_fluid: false,
                is_leaves: false,
            },
        );
        assert_eq!(get_empty_sections_for_chunk(&chunk), vec![Some(true); 24]);
    }
}
