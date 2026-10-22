mod beardifier_noise_parity {
    use super::*;
    use crate::levelgen::{
        noise::beardifier::{BeardifierPiece, BeardifierStart},
        structure::{BoundingBox, TerrainAdjustment},
    };
    use sha2::{Digest, Sha256};

    #[test]
    fn real_structure_terrain_noise_fill_matches_paper_blocks_and_heightmaps() {
        let (noise_registry, df_registry) = populated_registries();
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/beardifier-noise.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let pos = ChunkPos::new(
                row["x"].as_i64().unwrap() as i32,
                row["z"].as_i64().unwrap() as i32,
            );
            let mut settings = test_settings_with_final_density(fns::interpolated(
                fns::cache_once(fns::constant(-0.1)),
            ));
            settings.noise_settings = crate::levelgen::noise::noise_settings::create(48, 32, 1, 1);
            settings.default_fluid = Blocks::AIR.default_block_state();
            settings.sea_level = 0;
            let random = RandomState::create(
                &settings,
                &noise_registry,
                &df_registry,
                row["seed"].as_i64().unwrap(),
            );
            let generator = NoiseBasedChunkGenerator::new(Holder::Direct(settings));
            let factory = current_version_container_factory();
            let air = Blocks::AIR.default_block_state();
            let mut chunk: ProtoChunk<BlockState, BiomeId, &'static str> = ProtoChunk::new(
                pos,
                UpgradeData::empty(2),
                create_accessor(48, 32),
                &factory,
                None,
                air,
                air,
                &resolve_state_flags,
            );
            let adjustment = match row["adjustment"].as_str().unwrap() {
                "none" => TerrainAdjustment::None,
                "bury" => TerrainAdjustment::Bury,
                "beard_thin" => TerrainAdjustment::BeardThin,
                "beard_box" => TerrainAdjustment::BeardBox,
                "encapsulate" => TerrainAdjustment::Encapsulate,
                _ => panic!("unknown live fixture adjustment"),
            };
            let x = pos.get_min_block_x();
            let z = pos.get_min_block_z();
            let beardifier = Beardifier::from_ordered_starts(
                &[BeardifierStart {
                    terrain_adjustment: adjustment,
                    pieces: vec![BeardifierPiece::Ordinary {
                        bounds: BoundingBox::new([x + 2, 60, z + 2, x + 13, 68, z + 13]),
                    }],
                }],
                pos,
            );
            generator.fill_from_noise_with_structure_terrain(
                Blender::empty(),
                &random,
                &mut chunk,
                Arc::new(beardifier),
            );
            let mut hash = Sha256::new();
            let mut solid = 0;
            for y in 48..80 {
                for dz in 0..16 {
                    for dx in 0..16 {
                        let state = chunk.get_block_state(x + dx, y, z + dz);
                        hash.update((state.id().0 as i32).to_be_bytes());
                        if !state.is_air() {
                            solid += 1;
                        }
                    }
                }
            }
            assert_eq!(
                format!("{:x}", hash.finalize()),
                row["block_hash"].as_str().unwrap(),
                "blockhash {row}"
            );
            assert_eq!(
                solid,
                row["solid_count"].as_i64().unwrap(),
                "solid count {row}"
            );
            if adjustment != TerrainAdjustment::None {
                assert!(
                    solid > 0,
                    "actual beardifier must change negative-density all-air terrain"
                );
            } else {
                assert_eq!(solid, 0);
            }
            for (kind, name) in [
                (Types::WorldSurfaceWg, "WORLD_SURFACE_WG"),
                (Types::OceanFloorWg, "OCEAN_FLOOR_WG"),
            ] {
                let mut heights = Vec::new();
                for dz in 0..16 {
                    for dx in 0..16 {
                        heights.push(
                            chunk
                                .get_or_create_heightmap_unprimed(kind)
                                .get_height_at(dx, dz, 48),
                        );
                    }
                }
                assert_eq!(
                    serde_json::json!(heights),
                    row[name],
                    "heightmap {name} {row}"
                );
            }
            assert_eq!(
                chunk
                    .get_post_processing()
                    .iter()
                    .map(Vec::len)
                    .sum::<usize>() as u64,
                row["postprocessing"].as_u64().unwrap()
            );
        }
    }
}
