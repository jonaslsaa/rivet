use super::*;
use crate::levelgen::noise::density_function::{SinglePointContext, density_function_codec};
use rivet_registry::{
    RegistryBuilder, ResourceKey, access::RegistryAccess, registry_ops::RegistryOps,
};
use rivet_serialization::json_ops::JsonOps;
use serde_json::{Value, json};
use std::cell::Cell;

fn bounds(value: &Value) -> BoundingBox {
    BoundingBox::new(std::array::from_fn(|index| {
        value[index].as_i64().unwrap() as i32
    }))
}
fn adjustment(value: &Value) -> TerrainAdjustment {
    match value.as_str().unwrap() {
        "none" => TerrainAdjustment::None,
        "bury" => TerrainAdjustment::Bury,
        "beard_thin" => TerrainAdjustment::BeardThin,
        "beard_box" => TerrainAdjustment::BeardBox,
        "encapsulate" => TerrainAdjustment::Encapsulate,
        other => panic!("unknown actual Java adjustment {other}"),
    }
}
fn junction(value: &Value) -> BeardifierJunction {
    BeardifierJunction {
        source_x: value[0].as_i64().unwrap() as i32,
        source_ground_y: value[1].as_i64().unwrap() as i32,
        source_z: value[2].as_i64().unwrap() as i32,
    }
}
fn rigid(value: &Value) -> Rigid {
    Rigid {
        bounds: bounds(&value["bounds"]),
        terrain_adjustment: adjustment(&value["adjustment"]),
        ground_level_delta: value["ground_delta"].as_i64().unwrap() as i32,
    }
}
fn start(value: &Value) -> BeardifierStart {
    BeardifierStart {
        terrain_adjustment: adjustment(&value["adjustment"]),
        pieces: value["pieces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|piece| {
                let bounds = bounds(&piece["bounds"]);
                match piece["projection"].as_str() {
                    None => BeardifierPiece::Ordinary { bounds },
                    Some(name) => BeardifierPiece::Pool {
                        bounds,
                        projection: match name {
                            "rigid" => PoolProjection::Rigid,
                            "terrain_matching" => PoolProjection::TerrainMatching,
                            _ => panic!("unknown Java projection"),
                        },
                        ground_level_delta: piece["ground_delta"].as_i64().unwrap() as i32,
                        junctions: piece["junctions"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(junction)
                            .collect(),
                    },
                }
            })
            .collect(),
    }
}
struct Points {
    points: Vec<SinglePointContext>,
    fill_calls: Cell<usize>,
}
impl ContextProvider for Points {
    fn for_index(&self, index: usize) -> &dyn FunctionContext {
        &self.points[index]
    }
    fn fill_all_directly(&self, output: &mut [f64], function: &dyn DensityFunction) {
        self.fill_calls.set(self.fill_calls.get() + 1);
        for (index, value) in output.iter_mut().enumerate() {
            *value = function.compute(self.for_index(index));
        }
    }
}
#[test]
fn kernel_collector_compute_and_fill_match_actual_paper_bits() {
    for line in include_str!("../../../../../tools/rivet-oracle/fixtures/beardifier.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        if row["kind"] == "kernel" {
            let expected = row["bits"].as_array().unwrap();
            assert_eq!(expected.len(), KERNEL_VOLUME);
            for (index, (&value, bits)) in BEARD_KERNEL.iter().zip(expected).enumerate() {
                assert_eq!(
                    u64::from(value.to_bits()),
                    bits.as_u64().unwrap(),
                    "kernel index {index}"
                );
            }
            continue;
        }
        let expected = Beardifier::new(
            row["rigids"]
                .as_array()
                .unwrap()
                .iter()
                .map(rigid)
                .collect(),
            row["junctions"]
                .as_array()
                .unwrap()
                .iter()
                .map(junction)
                .collect(),
            (!row["affected"].is_null()).then(|| bounds(&row["affected"])),
        );
        let beardifier = if let Some(starts) = row["input"]["starts"].as_array() {
            let chunk = &row["input"]["chunk"];
            let starts: Vec<_> = starts.iter().map(start).collect();
            let collected = Beardifier::from_ordered_starts(
                &starts,
                ChunkPos::new(
                    chunk[0].as_i64().unwrap() as i32,
                    chunk[1].as_i64().unwrap() as i32,
                ),
            );
            assert_eq!(collected, expected, "collector {}", row["name"]);
            collected
        } else {
            expected
        };
        let provider = Points {
            points: row["points"]
                .as_array()
                .unwrap()
                .iter()
                .map(|point| {
                    SinglePointContext::new(
                        point[0].as_i64().unwrap() as i32,
                        point[1].as_i64().unwrap() as i32,
                        point[2].as_i64().unwrap() as i32,
                    )
                })
                .collect(),
            fill_calls: Cell::new(0),
        };
        for (index, point) in provider.points.iter().enumerate() {
            assert_eq!(
                beardifier.compute(point).to_bits(),
                row["bits"][index].as_str().unwrap().parse::<u64>().unwrap(),
                "compute {} {point:?}",
                row["name"]
            );
        }
        let mut filled = vec![f64::NAN; provider.points.len()];
        beardifier.fill_array(&mut filled, &provider);
        for (index, value) in filled.iter().enumerate() {
            assert_eq!(
                value.to_bits(),
                row["fill_bits"][index]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap(),
                "fill {} index{index}",
                row["name"]
            );
        }
        assert_eq!(
            provider.fill_calls.get() as u64,
            row["fill_calls"].as_u64().unwrap()
        );
        assert_eq!(
            beardifier.min_value().to_bits(),
            row["min_bits"].as_str().unwrap().parse::<u64>().unwrap()
        );
        assert_eq!(
            beardifier.max_value().to_bits(),
            row["max_bits"].as_str().unwrap().parse::<u64>().unwrap()
        );
        assert_eq!(
            beardifier
                .clone_arc()
                .compute(&provider.points[71])
                .to_bits(),
            beardifier.compute(&provider.points[71]).to_bits()
        );
    }
}

#[test]
fn missing_deferred_or_unordered_source_is_not_empty() {
    struct Source(BeardifierError);
    impl BeardifierSource for Source {
        fn starts_for_chunk(&self, _: ChunkPos) -> Result<Vec<BeardifierStart>, BeardifierError> {
            Err(self.0.clone())
        }
    }
    for error in [
        BeardifierError::ReferencesUnavailable(ChunkPos::ZERO),
        BeardifierError::DeferredInput("minecraft:village".into()),
        BeardifierError::OrderingUnavailable("reference order".into()),
        BeardifierError::UnsupportedPiece("minecraft:jigsaw metadata absent".into()),
    ] {
        assert_eq!(
            Beardifier::for_structures_in_chunk(&Source(error.clone()), ChunkPos::ZERO),
            Err(error)
        );
    }
}

#[test]
fn runtime_beardifier_encodes_as_unit_marker_without_downcast() {
    let density_key = &*super::super::registry_keys::DENSITY_FUNCTION;
    let registry = RegistryBuilder::new(density_key).freeze();
    let ops = RegistryOps::create_from_access(
        &JsonOps::INSTANCE,
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(density_key.identifier().clone()),
            Box::new(registry),
        )]),
    );
    let codec = density_function_codec();
    let function: Arc<dyn DensityFunction> = Arc::new(Beardifier::new(
        vec![Rigid {
            bounds: BoundingBox::new([0, 60, 0, 15, 80, 15]),
            terrain_adjustment: TerrainAdjustment::Bury,
            ground_level_delta: 1,
        }],
        Vec::new(),
        Some(BoundingBox::new([-24, 36, -24, 39, 104, 39])),
    ));
    let encoded = codec.encode_start(&ops, &function);
    assert_eq!(
        encoded.result(),
        Some(&json!({"type":"minecraft:beardifier"}))
    );
    let decoded = codec.parse(&ops, encoded.result().unwrap());
    assert!(
        decoded
            .result()
            .unwrap()
            .as_any()
            .is::<super::super::beardifier_marker::BeardifierMarker>()
    );
    assert_ne!(function.compute(&SinglePointContext::new(8, 61, 8)), 0.0);
}
