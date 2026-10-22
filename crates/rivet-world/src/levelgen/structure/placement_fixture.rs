use super::*;
use rivet_registry::{Holder, access::RegistryAccess, registry_ops::RegistryOps};
use rivet_serialization::json_ops::JsonOps;

#[test]
fn random_spread_and_all_frequency_methods_match_live_paper() {
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/structure-placement.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let int = |name: &str| i32::try_from(row[name].as_i64().unwrap()).unwrap();
        let seed = row["seed"].as_i64().unwrap();
        let x = int("x");
        let z = int("z");
        if row["kind"] == "spread" {
            let placement = RandomSpreadPlacement {
                settings: StructurePlacementSettings {
                    locate_offset: BlockPos::new(9, -4, 16),
                    salt: int("salt"),
                    ..Default::default()
                },
                spacing: int("spacing"),
                separation: int("separation"),
                spread_type: if row["spread_type"] == "linear" {
                    RandomSpreadType::Linear
                } else {
                    RandomSpreadType::Triangular
                },
            };
            let pos = placement.potential_structure_chunk(seed, x, z);
            assert_eq!(
                serde_json::json!([pos.x(), pos.z()]),
                row["potential"],
                "potential {row}"
            );
            let pos = placement.settings.locate_pos(pos);
            assert_eq!(
                serde_json::json!([pos.get_x(), pos.get_y(), pos.get_z()]),
                row["locate"],
                "locate {row}"
            );
        } else {
            let settings = StructurePlacementSettings {
                frequency_reduction_method: FrequencyReductionMethod::from_name(
                    row["method"].as_str().unwrap(),
                )
                .unwrap(),
                frequency: row["frequency"].as_f64().unwrap() as f32,
                salt: int("salt"),
                ..Default::default()
            };
            assert_eq!(
                settings.apply_additional_chunk_restrictions(
                    x,
                    z,
                    seed,
                    row["override"].as_i64().map(|n| i32::try_from(n).unwrap())
                ),
                row["result"].as_bool().unwrap(),
                "frequency {row}"
            );
        }
    }
}

#[test]
fn concentric_rings_match_live_paper_with_genuine_fixed_biome_search() {
    let plains = Holder::direct(BiomeId::from_name("minecraft:plains").unwrap());
    let desert = Holder::direct(BiomeId::from_name("minecraft:desert").unwrap());
    let source = crate::biome::fixed_biome_source::FixedBiomeSource::new(plains.clone());
    let sampler = crate::biome::climate::Climate::empty();
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/structure-rings.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let placement = ConcentricRingsPlacement {
            settings: Default::default(),
            distance: row["distance"].as_i64().unwrap() as i32,
            spread: row["spread"].as_i64().unwrap() as i32,
            count: row["count"].as_i64().unwrap() as i32,
            preferred_biomes: HolderSet::direct(vec![if row["matching"].as_bool().unwrap() {
                plains.clone()
            } else {
                desert.clone()
            }]),
        };
        let actual = generate_ring_positions_with_biome_source(
            &placement,
            row["seed"].as_i64().unwrap(),
            row["override"].as_i64(),
            &source,
            &sampler,
        )
        .unwrap();
        let expected: Vec<[i32; 2]> = serde_json::from_value(row["positions"].clone()).unwrap();
        assert_eq!(
            actual.iter().map(|p| [p.x(), p.z()]).collect::<Vec<_>>(),
            expected,
            "rings seed={} override={} matching={} config=({},{},{})",
            row["seed"],
            row["override"],
            row["matching"],
            placement.distance,
            placement.spread,
            placement.count
        );
    }
}

#[test]
fn placement_codec_preserves_exclusion_and_rejects_invalid_ranges() {
    let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
    let codec = structure_placement_codec();
    let valid = serde_json::json!({"type":"minecraft:random_spread","salt":0,"spacing":1,"separation":0,"exclusion_zone":{"other_set":"minecraft:villages","chunk_count":10}});
    let value = codec.parse(&ops, &valid).result().unwrap().clone();
    for alias in ["random_spread", ":random_spread"] {
        let mut aliased = valid.clone();
        aliased["type"] = serde_json::json!(alias);
        assert_eq!(codec.parse(&ops, &aliased).result(), Some(&value));
        assert_eq!(
            codec.encode_start(&ops, &value).result().unwrap()["type"],
            "minecraft:random_spread"
        );
    }
    let mut malformed = valid.clone();
    malformed["type"] = serde_json::json!("Minecraft:random_spread");
    assert!(codec.parse(&ops, &malformed).is_error());
    assert_eq!(
        value.settings().exclusion_zone.as_ref().unwrap().other_set,
        Identifier::parse("minecraft:villages")
    );
    for field in ["spacing", "separation", "salt"] {
        let mut invalid = valid.clone();
        invalid[field] = serde_json::json!(-1);
        assert!(
            codec.parse(&ops, &invalid).result().is_none(),
            "invalid {field}"
        );
    }
    let mut equal = valid.clone();
    equal["spacing"] = serde_json::json!(0);
    assert!(codec.parse(&ops, &equal).result().is_none());
    let mut offset = valid.clone();
    offset["locate_offset"] = serde_json::json!([16, 0, 0]);
    assert!(
        codec.parse(&ops, &offset).result().is_none(),
        "Vec3i offset bound is strict"
    );
    offset["locate_offset"] = serde_json::json!([i32::MIN, 0, 0]);
    assert!(
        codec.parse(&ops, &offset).result().is_some(),
        "Java Math.abs(MIN) stays negative"
    );
    let mut inline = valid;
    inline["exclusion_zone"]["other_set"] = serde_json::json!({"structures":[]});
    assert!(
        codec.parse(&ops, &inline).result().is_none(),
        "inline exclusions are disallowed"
    );
}

#[test]
fn exclusions_scan_in_source_order_and_never_suppress_missing_authority() {
    struct Authority {
        visited: Vec<(i32, i32)>,
        stop: Option<(i32, i32)>,
    }
    impl PlacementAuthority for Authority {
        fn is_structure_chunk(
            &mut self,
            _: &Identifier,
            x: i32,
            z: i32,
        ) -> Result<bool, PlacementError> {
            self.visited.push((x, z));
            Ok(self.stop == Some((x, z)))
        }
        fn ring_positions(
            &mut self,
            _: &ConcentricRingsPlacement,
        ) -> Result<Option<&[ChunkPos]>, PlacementError> {
            Err(PlacementError::AuthorityUnavailable(
                "no real ring cache".into(),
            ))
        }
    }
    let settings = StructurePlacementSettings {
        exclusion_zone: Some(ExclusionZone {
            other_set: Identifier::parse("minecraft:villages"),
            chunk_count: 1,
        }),
        ..Default::default()
    };
    let mut authority = Authority {
        visited: Vec::new(),
        stop: Some((0, -1)),
    };
    assert!(!settings.apply_interactions(&mut authority, 0, 0).unwrap());
    assert_eq!(authority.visited, [(-1, -1), (-1, 0), (-1, 1), (0, -1)]);
    let mut placement = StructurePlacement::RandomSpread(RandomSpreadPlacement {
        settings,
        spacing: 1,
        separation: 0,
        spread_type: RandomSpreadType::Linear,
    });
    placement.settings_mut().frequency = 0.0;
    authority.visited.clear();
    assert!(
        !placement
            .is_structure_chunk(&mut authority, 0, 0, 0, None)
            .unwrap()
    );
    assert!(
        authority.visited.is_empty(),
        "frequency rejection precedes exclusion queries"
    );
}
