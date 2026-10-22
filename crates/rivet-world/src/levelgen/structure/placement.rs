//! Pinned Paper structure-placement predicates and independent RNG streams.
//! Exclusions and biome-adjusted ring positions require real world authority.
use rivet_registry::{
    Identifier,
    biome_id::BiomeId,
    core::{BlockPos, ChunkPos},
    holder_set::HolderSet,
};
use rivet_util::{
    RandomSource, random_source::LegacyRandomSource, worldgen_random::WorldgenRandom,
};
#[path = "placement_codec.rs"]
mod placement_codec;
pub use placement_codec::structure_placement_codec;

#[cfg(test)]
#[path = "placement_fixture.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq)]
pub struct StructurePlacementSettings {
    pub locate_offset: BlockPos,
    pub frequency_reduction_method: FrequencyReductionMethod,
    pub frequency: f32,
    pub salt: i32,
    pub exclusion_zone: Option<ExclusionZone>,
}
impl Default for StructurePlacementSettings {
    fn default() -> Self {
        Self {
            locate_offset: BlockPos::ZERO,
            frequency_reduction_method: FrequencyReductionMethod::Default,
            frequency: 1.0,
            salt: 0,
            exclusion_zone: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusionZone {
    pub other_set: Identifier,
    pub chunk_count: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyReductionMethod {
    Default,
    LegacyType1,
    LegacyType2,
    LegacyType3,
}
impl FrequencyReductionMethod {
    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::LegacyType1 => "legacy_type_1",
            Self::LegacyType2 => "legacy_type_2",
            Self::LegacyType3 => "legacy_type_3",
        }
    }
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "default" => Some(Self::Default),
            "legacy_type_1" => Some(Self::LegacyType1),
            "legacy_type_2" => Some(Self::LegacyType2),
            "legacy_type_3" => Some(Self::LegacyType3),
            _ => None,
        }
    }
    pub fn should_generate(
        self,
        seed: i64,
        salt: i32,
        x: i32,
        z: i32,
        probability: f32,
        salt_override: Option<i32>,
    ) -> bool {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        match self {
            Self::Default => {
                // This surprising argument order is literal pinned Paper:
                // setLargeFeatureWithSalt(seed, salt, sourceX, sourceZ).
                random.set_large_feature_with_salt(seed, salt, x, z);
                random.next_float() < probability
            }
            Self::LegacyType1 => {
                random.set_seed(i64::from((x >> 4) ^ ((z >> 4).wrapping_shl(4))) ^ seed);
                random.next_int();
                random.next_int_bound((1.0f32 / probability) as i32) == 0
            }
            Self::LegacyType2 => {
                random.set_large_feature_with_salt(seed, x, z, salt_override.unwrap_or(10_387_320));
                random.next_float() < probability
            }
            Self::LegacyType3 => {
                if let Some(salt) = salt_override {
                    random.set_large_feature_with_salt(seed, x, z, salt);
                } else {
                    random.set_large_feature_seed(seed, x, z);
                }
                random.next_double() < f64::from(probability)
            }
        }
    }
}
impl StructurePlacementSettings {
    pub fn locate_pos(&self, chunk: ChunkPos) -> BlockPos {
        BlockPos::new(chunk.get_min_block_x(), 0, chunk.get_min_block_z()).offset(
            self.locate_offset.get_x(),
            self.locate_offset.get_y(),
            self.locate_offset.get_z(),
        )
    }
    pub fn apply_additional_chunk_restrictions(
        &self,
        x: i32,
        z: i32,
        seed: i64,
        salt_override: Option<i32>,
    ) -> bool {
        self.frequency.partial_cmp(&1.0) != Some(std::cmp::Ordering::Less)
            || self.frequency_reduction_method.should_generate(
                seed,
                self.salt,
                x,
                z,
                self.frequency,
                salt_override,
            )
    }
    pub fn apply_interactions(
        &self,
        authority: &mut impl PlacementAuthority,
        x: i32,
        z: i32,
    ) -> Result<bool, PlacementError> {
        let Some(zone) = &self.exclusion_zone else {
            return Ok(true);
        };
        // Java loops source-range .. source+range inclusively in X then Z.
        // An overflow-inverted interval is empty, not a saturating square.
        let mut test_x = x.wrapping_sub(zone.chunk_count);
        let end_x = x.wrapping_add(zone.chunk_count);
        while test_x <= end_x {
            let mut test_z = z.wrapping_sub(zone.chunk_count);
            let end_z = z.wrapping_add(zone.chunk_count);
            while test_z <= end_z {
                if authority.is_structure_chunk(&zone.other_set, test_x, test_z)? {
                    return Ok(false);
                }
                if test_z == i32::MAX {
                    return Err(PlacementError::CoordinateLoopOverflow);
                }
                test_z = test_z.wrapping_add(1);
            }
            if test_x == i32::MAX {
                return Err(PlacementError::CoordinateLoopOverflow);
            }
            test_x = test_x.wrapping_add(1);
        }
        Ok(true)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomSpreadType {
    Linear,
    Triangular,
}
impl RandomSpreadType {
    pub fn evaluate(self, random: &mut impl RandomSource, limit: i32) -> i32 {
        match self {
            Self::Linear => random.next_int_bound(limit),
            Self::Triangular => {
                (random
                    .next_int_bound(limit)
                    .wrapping_add(random.next_int_bound(limit)))
                    / 2
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct RandomSpreadPlacement {
    pub settings: StructurePlacementSettings,
    pub spacing: i32,
    pub separation: i32,
    pub spread_type: RandomSpreadType,
}
impl RandomSpreadPlacement {
    pub fn potential_structure_chunk(&self, seed: i64, x: i32, z: i32) -> ChunkPos {
        let grid_x = x.div_euclid(self.spacing);
        let grid_z = z.div_euclid(self.spacing);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_with_salt(seed, grid_x, grid_z, self.settings.salt);
        let limit = self.spacing.wrapping_sub(self.separation);
        let dx = self.spread_type.evaluate(&mut random, limit);
        let dz = self.spread_type.evaluate(&mut random, limit);
        ChunkPos::new(
            grid_x.wrapping_mul(self.spacing).wrapping_add(dx),
            grid_z.wrapping_mul(self.spacing).wrapping_add(dz),
        )
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ConcentricRingsPlacement {
    pub settings: StructurePlacementSettings,
    pub distance: i32,
    pub spread: i32,
    pub count: i32,
    pub preferred_biomes: HolderSet<BiomeId>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum StructurePlacement {
    RandomSpread(RandomSpreadPlacement),
    ConcentricRings(ConcentricRingsPlacement),
}
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum PlacementError {
    #[error("structure placement authority unavailable: {0}")]
    AuthorityUnavailable(String),
    #[error("Java exclusion scan would wrap indefinitely at maximum coordinates")]
    CoordinateLoopOverflow,
}
/// Resolve actual other-set predicates (including their own frequency and
/// exclusion checks) and actual generated ring caches. Missing authority must
/// return an error, not false or unadjusted synthetic ring positions.
pub trait PlacementAuthority {
    fn is_structure_chunk(
        &mut self,
        set: &Identifier,
        x: i32,
        z: i32,
    ) -> Result<bool, PlacementError>;
    fn ring_positions(
        &mut self,
        placement: &ConcentricRingsPlacement,
    ) -> Result<Option<&[ChunkPos]>, PlacementError>;
}
impl StructurePlacement {
    pub fn settings(&self) -> &StructurePlacementSettings {
        match self {
            Self::RandomSpread(p) => &p.settings,
            Self::ConcentricRings(p) => &p.settings,
        }
    }
    pub fn settings_mut(&mut self) -> &mut StructurePlacementSettings {
        match self {
            Self::RandomSpread(p) => &mut p.settings,
            Self::ConcentricRings(p) => &mut p.settings,
        }
    }
    pub fn is_structure_chunk(
        &self,
        authority: &mut impl PlacementAuthority,
        seed: i64,
        x: i32,
        z: i32,
        salt_override: Option<i32>,
    ) -> Result<bool, PlacementError> {
        let placed = match self {
            Self::RandomSpread(p) => p.potential_structure_chunk(seed, x, z) == ChunkPos::new(x, z),
            Self::ConcentricRings(p) => authority
                .ring_positions(p)?
                .is_some_and(|positions| positions.contains(&ChunkPos::new(x, z))),
        };
        if !placed
            || !self
                .settings()
                .apply_additional_chunk_restrictions(x, z, seed, salt_override)
        {
            return Ok(false);
        }
        self.settings().apply_interactions(authority, x, z)
    }
}

/// Synchronous equivalent of the independent generateRingPositions tasks.
/// Each biome search receives the exact forked Legacy stream and must perform
/// the genuine horizontal search (112 blocks at y0) or return unavailable.
pub fn generate_ring_positions(
    placement: &ConcentricRingsPlacement,
    concentric_seed: i64,
    stronghold_seed: Option<i64>,
    mut biome_search: impl FnMut(
        BlockPos,
        i32,
        &HolderSet<BiomeId>,
        &mut LegacyRandomSource,
    ) -> Result<Option<BlockPos>, PlacementError>,
) -> Result<Vec<ChunkPos>, PlacementError> {
    if placement.count == 0 {
        return Ok(Vec::new());
    }
    let mut random = LegacyRandomSource::new(stronghold_seed.unwrap_or(concentric_seed));
    let mut angle = random.next_double() * std::f64::consts::PI * 2.0;
    let mut in_circle = 0i32;
    let mut circle = 0i32;
    let mut spread = placement.spread;
    let mut positions = Vec::with_capacity(placement.count as usize);
    for i in 0..placement.count {
        let distance = placement.distance;
        let dist = f64::from(
            4i32.wrapping_mul(distance)
                .wrapping_add(distance.wrapping_mul(circle).wrapping_mul(6)),
        ) + (random.next_double() - 0.5) * (f64::from(distance) * 2.5);
        let x = ((angle.cos() * dist + 0.5).floor() as i64) as i32;
        let z = ((angle.sin() * dist + 0.5).floor() as i64) as i32;
        let mut search = random.fork();
        let origin = BlockPos::new(
            x.wrapping_shl(4).wrapping_add(8),
            0,
            z.wrapping_shl(4).wrapping_add(8),
        );
        let target = biome_search(origin, 112, &placement.preferred_biomes, &mut search)?;
        positions.push(target.map_or(ChunkPos::new(x, z), |pos| {
            ChunkPos::new(pos.get_x() >> 4, pos.get_z() >> 4)
        }));
        angle += (std::f64::consts::PI * 2.0) / f64::from(spread);
        in_circle = in_circle.wrapping_add(1);
        if in_circle == spread {
            circle = circle.wrapping_add(1);
            in_circle = 0;
            spread = spread.wrapping_add(2i32.wrapping_mul(spread) / (circle + 1));
            spread = spread.min(placement.count - i);
            angle += random.next_double() * std::f64::consts::PI * 2.0;
        }
    }
    Ok(positions)
}

pub fn generate_ring_positions_with_biome_source(
    placement: &ConcentricRingsPlacement,
    concentric_seed: i64,
    stronghold_seed: Option<i64>,
    source: &dyn crate::biome::biome_source::BiomeSource,
    sampler: &crate::biome::climate::Sampler,
) -> Result<Vec<ChunkPos>, PlacementError> {
    generate_ring_positions(
        placement,
        concentric_seed,
        stronghold_seed,
        |origin, radius, preferred, random| {
            Ok(crate::biome::biome_source::find_biome_horizontal(
                source,
                origin.get_x(),
                0,
                origin.get_z(),
                radius,
                &|biome| preferred.contains(&biome),
                random,
                sampler,
            )
            .map(|(pos, _)| pos))
        },
    )
}

/// Explicit Paper/Spigot world settings. No global mutable config or inferred
/// custom seed; the defaults are pinned SpigotWorldConfig.initWorldGenSeeds.
#[derive(Debug, Clone)]
pub struct PaperStructureSeeds {
    pub village: i32,
    pub desert: i32,
    pub igloo: i32,
    pub jungle: i32,
    pub swamp: i32,
    pub monument: i32,
    pub shipwreck: i32,
    pub ocean: i32,
    pub outpost: i32,
    pub end_city: i32,
    pub nether: i32,
    pub mansion: i32,
    pub fossil: i32,
    pub portal: i32,
    pub ancient_city: i32,
    pub trail_ruins: i32,
    pub trial_chambers: i32,
    pub buried_treasure: i32,
    pub mineshaft: Option<i32>,
    pub stronghold: Option<i64>,
}
impl Default for PaperStructureSeeds {
    fn default() -> Self {
        Self {
            village: 10_387_312,
            desert: 14_357_617,
            igloo: 14_357_618,
            jungle: 14_357_619,
            swamp: 14_357_620,
            monument: 10_387_313,
            shipwreck: 165_745_295,
            ocean: 14_357_621,
            outpost: 165_745_296,
            end_city: 10_387_313,
            nether: 30_084_232,
            mansion: 10_387_319,
            fossil: 14_357_921,
            portal: 34_222_645,
            ancient_city: 20_083_232,
            trail_ruins: 83_469_867,
            trial_chambers: 94_251_327,
            buried_treasure: 10_387_320,
            mineshaft: None,
            stronghold: None,
        }
    }
}
impl PaperStructureSeeds {
    pub fn inject_random_spread(&self, key: &Identifier, placement: &mut StructurePlacement) {
        if key.namespace() != "minecraft" {
            return;
        }
        let StructurePlacement::RandomSpread(placement) = placement else {
            return;
        };
        placement.settings.salt = match key.path() {
            "villages" => self.village,
            "desert_pyramids" => self.desert,
            "igloos" => self.igloo,
            "jungle_temples" => self.jungle,
            "swamp_huts" => self.swamp,
            "ocean_monuments" => self.monument,
            "shipwrecks" => self.shipwreck,
            "ocean_ruins" => self.ocean,
            "pillager_outposts" => self.outpost,
            "end_cities" => self.end_city,
            "nether_complexes" => self.nether,
            "woodland_mansions" => self.mansion,
            "nether_fossils" => self.fossil,
            "ruined_portals" => self.portal,
            "ancient_cities" => self.ancient_city,
            "trail_ruins" => self.trail_ruins,
            "trial_chambers" => self.trial_chambers,
            _ => placement.settings.salt,
        };
    }
    pub fn frequency_override(&self, key: Option<&Identifier>) -> Option<i32> {
        let key = key?;
        if key.namespace() != "minecraft" {
            return None;
        }
        match key.path() {
            "mineshafts" => self.mineshaft,
            "buried_treasures" => Some(self.buried_treasure),
            _ => None,
        }
    }
    pub fn ring_override(&self, key: &Identifier) -> Option<i64> {
        if key.namespace() == "minecraft" && key.path() == "strongholds" {
            self.stronghold
        } else {
            None
        }
    }
}
