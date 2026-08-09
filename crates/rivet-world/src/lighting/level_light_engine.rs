//! `net.minecraft.world.level.lighting.LevelLightEngine` — the light engine
//! facade (MC 26.2, Paper).
//!
//! Java source: `LevelLightEngine.java` in the mache base. The class implements
//! `LightEventListener`, holding a `LevelHeightAccessor` plus (in vanilla) two
//! `LightEngine<?, ?>` members (`blockEngine`/`skyEngine`). Paper's chunk
//! rewrite replaces those engines with Starlight: `ThreadedLevelLightEngine` (a
//! server-layer subclass) implements `StarLightLightingProvider` and routes
//! `checkBlock`/`updateSectionStatus` to
//! `starlight$getLightEngine().blockChange()`/`sectionChange()`.
//!
//! The port flattens that into the provider seam:
//! [`StarLightProvider`](crate::lighting::star_light_provider). This facade
//! owns the world's vertical extent (`Box<dyn LevelHeightAccessor>` — Java
//! obtains it from the constructor's `LightChunkGetter.getLevel()`, taken
//! directly here so the generic getter does not force a block-state type
//! parameter onto the facade) and an `Option<Box<dyn StarLightProvider + Send>>`
//! standing in for the two `LightEngine<?,?>` fields. The facade is the
//! compile-green skeleton the later phases build on: the `LightEventListener`
//! delegation surface and `getLayerListener` land with the
//! `mc.world.level.lighting.core` unit.
//!
//! The `EMPTY` singleton (Java `new LevelLightEngine()` with no engines) is
//! [`Default`].
//!
//! RivetTodo(#184): the `LightEngine`/`LayerLightSectionStorage` engine surface
//! and the `LightEventListener`/`getLayerListener` implementation are not
//! ported (the `mc.world.level.lighting.core`/`.engine` units); this module is
//! the facade skeleton — section arithmetic, light flags, and the provider
//! holder.

use crate::level::height_accessor::LevelHeightAccessor;
use crate::lighting::star_light_provider::StarLightProvider;

/// `LevelLightEngine.LIGHT_SECTION_PADDING` — each light-section extent adds
/// one padding section beyond the world's build sections.
pub const LIGHT_SECTION_PADDING: i32 = 1;

/// `net.minecraft.world.level.lighting.LevelLightEngine`.
///
/// Ownership mirrors OWNERSHIP.md: the facade owns its accessor and provider by
/// value (no `Arc<RwLock>`), hands out `&mut` for mutation, and is `Send` but
/// never `Sync` — a shared `&LevelLightEngine` cannot cross threads, the same
/// confinement as the world that will own it.
pub struct LevelLightEngine {
    /// `levelHeightAccessor` — the world's vertical extent (Java gets it from
    /// the `LightChunkGetter`).
    level_height_accessor: Box<dyn LevelHeightAccessor + Send>,
    /// The concrete light provider (`StarLightInterface` in `rivet-server`),
    /// replacing Java's `blockEngine`/`skyEngine` `LightEngine<?,?>` members.
    provider: Option<Box<dyn StarLightProvider + Send>>,
    /// `hasBlockLight` — whether a block-light engine is active.
    has_block_light: bool,
    /// `hasSkyLight` — whether a sky-light engine is active.
    has_sky_light: bool,
}

impl LevelLightEngine {
    /// `LevelLightEngine(LightChunkGetter, boolean hasBlockLight, boolean
    /// hasSkyLight)` — the facade with no provider attached yet (the provider
    /// is the `rivet-server` Starlight impl; [`Self::with_provider`] attaches
    /// it). `level_height_accessor` is the `getLevel()` the Java constructor
    /// reads from the `LightChunkGetter`.
    pub fn new(
        level_height_accessor: Box<dyn LevelHeightAccessor + Send>,
        has_block_light: bool,
        has_sky_light: bool,
    ) -> Self {
        LevelLightEngine {
            level_height_accessor,
            provider: None,
            has_block_light,
            has_sky_light,
        }
    }

    /// `new` with the cycle-breaking provider attached — the seam
    /// `rivet-server` uses to exercise `StarLightInterface` end to end.
    pub fn with_provider(
        level_height_accessor: Box<dyn LevelHeightAccessor + Send>,
        has_block_light: bool,
        has_sky_light: bool,
        provider: Box<dyn StarLightProvider + Send>,
    ) -> Self {
        LevelLightEngine {
            level_height_accessor,
            provider: Some(provider),
            has_block_light,
            has_sky_light,
        }
    }

    /// `LevelLightEngine.getLightSectionCount()` — `getSectionsCount() + 2`,
    /// one padding section above and below the world's sections.
    pub fn get_light_section_count(&self) -> i32 {
        self.level_height_accessor.get_sections_count() + 2 * LIGHT_SECTION_PADDING
    }

    /// `LevelLightEngine.getMinLightSection()` — `getMinSectionY() - 1`.
    pub fn get_min_light_section(&self) -> i32 {
        self.level_height_accessor.get_min_section_y() - LIGHT_SECTION_PADDING
    }

    /// `LevelLightEngine.getMaxLightSection()` — `getMinLightSection() +
    /// getLightSectionCount()` (Java's *exclusive* upper bound: the highest
    /// section is `getMaxLightSection() - 1`).
    pub fn get_max_light_section(&self) -> i32 {
        self.get_min_light_section() + self.get_light_section_count()
    }

    /// Whether a block-light engine is active (`hasBlockLight`).
    pub fn has_block_light(&self) -> bool {
        self.has_block_light
    }

    /// Whether a sky-light engine is active (`hasSkyLight`).
    pub fn has_sky_light(&self) -> bool {
        self.has_sky_light
    }

    /// The world's vertical extent (Java's protected `levelHeightAccessor`).
    pub fn level_height_accessor(&self) -> &dyn LevelHeightAccessor {
        self.level_height_accessor.as_ref()
    }

    /// The attached provider, or `None` when the facade was built without one.
    pub fn provider(&self) -> Option<&dyn StarLightProvider> {
        self.provider
            .as_deref()
            .map(|p| p as &dyn StarLightProvider)
    }

    /// The attached provider for exclusive mutation — the seam's `&mut` owner.
    pub fn provider_mut(&mut self) -> Option<&mut dyn StarLightProvider> {
        self.provider
            .as_deref_mut()
            .map(|p| p as &mut dyn StarLightProvider)
    }
}

/// Java's `EMPTY` singleton — `new LevelLightEngine()` with
/// `LevelHeightAccessor.create(0, 0)` and no engines/provider.
impl Default for LevelLightEngine {
    fn default() -> Self {
        LevelLightEngine::new(
            Box::new(crate::level::height_accessor::create(0, 0)),
            false,
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::create as create_accessor;
    use rivet_registry::core::SectionPos;

    /// The overworld superflat accessor (minY -64, height 384, 24 sections).
    fn overworld() -> Box<dyn LevelHeightAccessor + Send> {
        Box::new(create_accessor(-64, 384))
    }

    #[test]
    fn overworld_light_section_bounds_match_java() {
        // `LevelLightEngine` on the overworld: minSectionY = -4, sectionsCount
        // = 24, so getLightSectionCount() = 26, getMinLightSection() = -5, and
        // getMaxLightSection() = -5 + 26 = 21 (the exclusive upper bound).
        let engine = LevelLightEngine::new(overworld(), true, true);
        assert_eq!(engine.get_light_section_count(), 26);
        assert_eq!(engine.get_min_light_section(), -5);
        assert_eq!(engine.get_max_light_section(), 21);
        assert!(engine.has_block_light());
        assert!(engine.has_sky_light());
    }

    #[test]
    fn light_flags_follow_the_constructor_arguments() {
        let block_only = LevelLightEngine::new(overworld(), true, false);
        assert!(block_only.has_block_light());
        assert!(!block_only.has_sky_light());
        let sky_only = LevelLightEngine::new(overworld(), false, true);
        assert!(!sky_only.has_block_light());
        assert!(sky_only.has_sky_light());
    }

    #[test]
    fn empty_default_matches_the_java_singleton() {
        // Java `EMPTY = new LevelLightEngine()`: LevelHeightAccessor.create(0,
        // 0) (minY 0, height 0), no engines. minSectionY = 0, sectionsCount = 0,
        // so lightSectionCount = 2, minLightSection = -1, maxLightSection = 1.
        let mut empty = LevelLightEngine::default();
        assert_eq!(empty.get_light_section_count(), 2);
        assert_eq!(empty.get_min_light_section(), -1);
        assert_eq!(empty.get_max_light_section(), 1);
        assert!(!empty.has_block_light());
        assert!(!empty.has_sky_light());
        assert!(empty.provider().is_none());
        assert!(empty.provider_mut().is_none());
        // The empty accessor's own section math: get_min_section_y() = 0.
        assert_eq!(empty.level_height_accessor().get_min_section_y(), 0);
    }

    #[test]
    fn section_math_never_panics_on_extreme_extents() {
        // The facade computes Java's section arithmetic with plain `+`/`-`
        // (PORTING.md wrapping semantics). This world height (i32::MAX) never
        // overflows any intermediate value: maxY = i32::MAX - 1, sectionsCount
        // = 134217728, so the light-section bounds are exactly Java's. The
        // section values are asserted below; the wrap-vs-saturate counterfactual
        // (an input that genuinely overflows) is `section_math_wraps_on_overflowing_height`.
        let huge = LevelLightEngine::new(Box::new(create_accessor(0, i32::MAX)), false, false);
        // (i32::MAX - 1) >> 4 = 134217727 sections + 2 padding.
        assert_eq!(huge.get_light_section_count(), 134_217_728 + 2);
        assert_eq!(huge.get_min_light_section(), -1);
        assert_eq!(huge.get_max_light_section(), 134_217_727 + 2);
    }

    #[test]
    fn section_math_wraps_on_overflowing_height() {
        // The genuine wrap-vs-saturate counterfactual: Java's plain `+`
        // overflows in `getMaxY()` (minY + height - 1 = 3_000_000_000 - 1 >
        // i32::MAX), so the wrapped section bounds differ from what saturating
        // arithmetic would produce. The facade must reproduce Java's wrapped
        // values (and no panic) — not a saturating "improvement".
        // create(1.5e9, 1.5e9): maxY wraps to -1294967297,
        // minSectionY = 93750000, maxSectionY = -80935457, sectionsCount =
        // -174685456, lightSectionCount = -174685454, minLightSection =
        // 93749999, maxLightSection = -80935455.
        let wrapped = LevelLightEngine::new(
            Box::new(create_accessor(1_500_000_000, 1_500_000_000)),
            false,
            false,
        );
        assert_eq!(wrapped.get_light_section_count(), -174_685_454);
        assert_eq!(wrapped.get_min_light_section(), 93_749_999);
        assert_eq!(wrapped.get_max_light_section(), -80_935_455);
    }

    #[test]
    fn provider_mut_is_the_exclusive_delegation_seam() {
        use crate::lighting::star_light_provider::StarLightProvider;
        use rivet_registry::core::{BlockPos, ChunkPos, Vec3iLike};
        use std::collections::HashSet;
        use std::sync::{Arc, Mutex};

        // The recording provider observes through an `Arc<Mutex>` so the test
        // can read what the facade's `&mut dyn` calls actually reached. Each
        // entry is the op name plus the absolute coordinates it carried.
        type LogEntry = (String, (i32, i32, i32));
        let log: Arc<Mutex<Vec<LogEntry>>> = Arc::new(Mutex::new(Vec::new()));
        struct Recording(Arc<Mutex<Vec<LogEntry>>>);
        impl StarLightProvider for Recording {
            fn block_change(&mut self, pos: BlockPos) {
                self.0.lock().unwrap().push(("block".into(), pos.coords()));
            }
            fn section_change(&mut self, pos: SectionPos, new_empty_value: bool) {
                self.0
                    .lock()
                    .unwrap()
                    .push((format!("section:{new_empty_value}"), pos.coords()));
            }
            fn light_chunk(&mut self, _pos: ChunkPos, _empty_sections: &[Option<bool>]) {}
            fn relight_chunks(&mut self, _chunks: &HashSet<ChunkPos>) {}
            fn check_chunk_edges(&mut self, _pos: ChunkPos) {}
            fn get_sky_light_value(&self, _pos: BlockPos) -> i32 {
                0
            }
            fn get_block_light_value(&self, _pos: BlockPos) -> i32 {
                0
            }
            fn get_data_layer_data(
                &self,
                _pos: SectionPos,
            ) -> Option<crate::chunk::data_layer::DataLayer> {
                None
            }
        }

        let mut engine = LevelLightEngine::with_provider(
            overworld(),
            true,
            true,
            Box::new(Recording(Arc::clone(&log))),
        );
        let pos = BlockPos::new(3, 64, 9);
        engine.provider_mut().expect("attached").block_change(pos);
        engine
            .provider_mut()
            .expect("attached")
            .section_change(SectionPos::of(0, 4, 0), true);
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                ("block".into(), (3, 64, 9)),
                ("section:true".into(), (0, 4, 0)),
            ]
        );
        // The provider is reachable read-only, and only while the facade owns it.
        assert!(engine.provider().is_some());
    }

    #[test]
    fn facade_is_send_but_not_sync_single_owner_confinement() {
        // OWNERSHIP "one owner: the tick thread": the facade may move onto the
        // tick thread (Send) but a shared `&LevelLightEngine` must never cross
        // threads (!Sync), matching the world that will own it (server_level.rs
        // asserts the same for `ServerLevel`).
        fn assert_send<T: Send>() {}
        assert_send::<LevelLightEngine>();
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            struct Invalid;
            impl<T: ?Sized + Sync> AmbiguousIfImpl<Invalid> for T {}
            // Resolves only if `LevelLightEngine` does NOT implement Sync.
            let _ = <LevelLightEngine as AmbiguousIfImpl<_>>::some_item;
        };
    }
}
