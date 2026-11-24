//! The `shapes.eval` corpus, committed as `corpus/shapes.eval.jsonl`.
//!
//! Four sources, all deterministic (a pure function of [`CORPUS_SEED`](super::packet_corpus::CORPUS_SEED)
//! and the committed vanilla shape list), none carrying an expected value:
//!
//! 1. `Vec3` functions over special and seeded random vectors (infinities,
//!    NaN, signed zeros, subnormals, values straddling `1.0E-5F`), including
//!    the float-widening rotation paths (`xRot`/`yRot`/`zRot`/`rotation`).
//! 2. `AABB` functions over random and boundary boxes (`move`, `inflate`,
//!    `intersects`, `clip`, `distanceToSqr`, ...).
//! 3. `Shapes`/`VoxelShape` over a catalog of hand-shaped and random shapes:
//!    `Shapes.box`/`create`/`or`, `join`/`joinUnoptimized` with every
//!    `BooleanOp`, `collide` per axis, `optimize`, `getFaceShape`, `toAabbs`,
//!    occlusion and the `Shapes.equal`/`joinIsNotEmpty` queries.
//! 4. Every distinct vanilla block-state collision, outline, occlusion and
//!    visual shape (`corpus-src/vanilla_shapes.jsonl`, harvested from Paper by
//!    `run.sh --dump-vanilla-shapes`), rebuilt from its box list.
//!
//! Paper answers every input.

use serde_json::{Map, Value, json};

use super::ops_dir;
use super::packet_corpus::Rng;
use super::shapes::{f32_bits, f64_bits};

const BOOLEAN_OPS: [&str; 16] = [
    "FALSE",
    "NOT_OR",
    "ONLY_SECOND",
    "NOT_FIRST",
    "ONLY_FIRST",
    "NOT_SECOND",
    "NOT_SAME",
    "NOT_AND",
    "AND",
    "SAME",
    "SECOND",
    "CAUSES",
    "FIRST",
    "CAUSED_BY",
    "OR",
    "TRUE",
];
const DIRECTIONS: [&str; 6] = ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"];
const AXES: [&str; 3] = ["X", "Y", "Z"];

fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

fn pick<T: Copy>(rng: &mut Rng, items: &[T]) -> T {
    items[rng.below(items.len() as u64) as usize]
}

/// Doubles that stress rounding, signs, magnitudes and non-finite values.
const SPECIAL: [f64; 22] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    0.1,
    0.3,
    1.0E-7,
    9.999999E-8,
    -1.0E-7,
    1.0E-8,
    0.9999999,
    1.0000001,
    1.0000002,
    1.0E-5,
    100.0,
    -1.0E300,
    1.0E300,
    f64::MIN_POSITIVE,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
];

/// A coordinate: special 1 time in 4, otherwise a signed random value of a
/// random magnitude.
fn coordinate(rng: &mut Rng) -> f64 {
    if rng.below(4) == 0 {
        pick(rng, &SPECIAL)
    } else {
        let magnitude = [0.5, 2.0, 16.0, 4096.0][rng.below(4) as usize];
        (unit(rng) * 2.0 - 1.0) * magnitude
    }
}

/// A finite coordinate near the unit cube (a block-local position).
fn local(rng: &mut Rng) -> f64 {
    match rng.below(3) {
        0 => rng.below(17) as f64 / 16.0,
        1 => rng.below(9) as f64 / 8.0 - 0.25,
        _ => unit(rng) * 1.6 - 0.3,
    }
}

fn h(x: f64) -> Value {
    json!(f64_bits(x))
}

fn vec3_value(v: [f64; 3]) -> Value {
    json!([h(v[0]), h(v[1]), h(v[2])])
}

fn box_value(b: [f64; 6]) -> Value {
    json!([h(b[0]), h(b[1]), h(b[2]), h(b[3]), h(b[4]), h(b[5])])
}

fn random_vec3(rng: &mut Rng) -> [f64; 3] {
    [coordinate(rng), coordinate(rng), coordinate(rng)]
}

fn local_vec3(rng: &mut Rng) -> [f64; 3] {
    [local(rng), local(rng), local(rng)]
}

fn random_box(rng: &mut Rng) -> [f64; 6] {
    [
        coordinate(rng),
        coordinate(rng),
        coordinate(rng),
        coordinate(rng),
        coordinate(rng),
        coordinate(rng),
    ]
}

fn local_box(rng: &mut Rng) -> [f64; 6] {
    [
        local(rng),
        local(rng),
        local(rng),
        local(rng),
        local(rng),
        local(rng),
    ]
}

fn float32(rng: &mut Rng) -> f32 {
    const SPECIAL_F32: [f32; 9] = [
        0.0,
        -0.0,
        std::f32::consts::FRAC_PI_2,
        std::f32::consts::PI,
        -1.0,
        1.0E-8,
        f32::INFINITY,
        f32::NAN,
        1.0E-5,
    ];
    if rng.below(3) == 0 {
        pick(rng, &SPECIAL_F32)
    } else {
        ((unit(rng) * 2.0 - 1.0) * 7.0) as f32
    }
}

struct Lines(String);

impl Lines {
    fn push(&mut self, id: String, fields: Value) {
        let mut object = Map::new();
        object.insert("id".into(), json!(id));
        let Value::Object(fields) = fields else {
            unreachable!("corpus fields are objects")
        };
        object.extend(fields);
        self.0 += &(serde_json::to_string(&Value::Object(object))
            .expect("JSON values always serialize")
            + "\n");
    }
}

fn vec3_corpus(rng: &mut Rng, out: &mut Lines) {
    // (fn, uses b, uses s, uses f/g)
    const FNS: &[(&str, bool, bool, bool)] = &[
        ("add", true, false, false),
        ("addScalar", false, true, false),
        ("subtract", true, false, false),
        ("subtractScalar", false, true, false),
        ("multiply", true, false, false),
        ("scale", false, true, false),
        ("cross", true, false, false),
        ("vectorTo", true, false, false),
        ("normalize", false, false, false),
        ("reverse", false, false, false),
        ("horizontal", false, false, false),
        ("lerp", true, true, false),
        ("xRot", false, false, true),
        ("yRot", false, false, true),
        ("zRot", false, false, true),
        ("rotateClockwise90", false, false, false),
        ("projectedOn", true, false, false),
        ("addLocalCoordinates", true, false, false),
        ("directionFromRotation", false, false, true),
        ("dot", true, false, false),
        ("length", false, false, false),
        ("lengthSqr", false, false, false),
        ("distanceTo", true, false, false),
        ("distanceToSqr", true, false, false),
        ("horizontalDistance", false, false, false),
        ("horizontalDistanceSqr", false, false, false),
        ("closerThan", true, true, false),
        ("closerThanXzY", true, true, false),
        ("isFinite", false, false, false),
        ("equals", true, false, false),
        ("hashCode", false, false, false),
        ("toString", false, false, false),
        ("rotation", false, false, false),
    ];
    for &(name, uses_b, uses_s, uses_f) in FNS {
        for n in 0..24 {
            let mut fields = Map::new();
            fields.insert("kind".into(), json!("vec3"));
            fields.insert("fn".into(), json!(name));
            // The first cases are all-special so every function meets zero,
            // infinities and NaN; the rest are seeded random vectors, with a
            // short vector (norm below 1.0E-5F) every fifth case.
            let (a, b) = if n < 6 {
                (
                    [
                        pick(rng, &SPECIAL),
                        pick(rng, &SPECIAL),
                        pick(rng, &SPECIAL),
                    ],
                    [
                        pick(rng, &SPECIAL),
                        pick(rng, &SPECIAL),
                        pick(rng, &SPECIAL),
                    ],
                )
            } else if n % 5 == 0 {
                (
                    [unit(rng) * 1.0E-5, unit(rng) * 1.0E-6, 0.0],
                    random_vec3(rng),
                )
            } else {
                (random_vec3(rng), random_vec3(rng))
            };
            fields.insert("a".into(), vec3_value(a));
            if uses_b {
                fields.insert("b".into(), vec3_value(b));
            }
            if uses_s {
                let s = if n < 6 { pick(rng, &SPECIAL) } else { unit(rng) * 3.0 - 1.0 };
                fields.insert("s".into(), h(s));
                if name == "closerThanXzY" {
                    fields.insert("t".into(), h(unit(rng) * 5.0));
                }
            }
            if uses_f {
                fields.insert("f".into(), json!(f32_bits(float32(rng))));
                fields.insert("g".into(), json!(f32_bits(float32(rng))));
            }
            out.push(format!("vec3.{name}.{n}"), Value::Object(fields));
        }
    }
}

fn aabb_corpus(rng: &mut Rng, out: &mut Lines) {
    const FNS: &[&str] = &[
        "new",
        "move",
        "moveVec",
        "moveBlockPos",
        "inflate3",
        "inflate",
        "deflate3",
        "deflate",
        "contract",
        "expandTowards",
        "expandTowards3",
        "intersect",
        "minmax",
        "setMinX",
        "setMaxY",
        "setMinZ",
        "intersects",
        "intersectsVecs",
        "intersectsBlockPos",
        "contains",
        "hasNaN",
        "equals",
        "hashCode",
        "toString",
        "getSize",
        "getXsize",
        "getYsize",
        "getZsize",
        "distanceToSqrVec",
        "distanceToSqrBox",
        "getCenter",
        "getBottomCenter",
        "getMinPosition",
        "getMaxPosition",
        "clip",
        "clipAll",
        "collidedAlongVector",
        "ofSize",
    ];
    for &name in FNS {
        for n in 0..24 {
            let mut fields = Map::new();
            fields.insert("kind".into(), json!("aabb"));
            fields.insert("fn".into(), json!(name));
            // Boundary cases first (touching boxes, point boxes, NaN, infinity),
            // then local boxes (the block-sized scale the game works at), then
            // wide random ones.
            let b = if n < 3 {
                [
                    pick(rng, &SPECIAL),
                    pick(rng, &SPECIAL),
                    pick(rng, &SPECIAL),
                    pick(rng, &SPECIAL),
                    pick(rng, &SPECIAL),
                    pick(rng, &SPECIAL),
                ]
            } else if n < 18 {
                local_box(rng)
            } else {
                random_box(rng)
            };
            fields.insert("box".into(), box_value(b));
            let second = if n % 2 == 0 { local_box(rng) } else { random_box(rng) };
            fields.insert("box2".into(), box_value(second));
            let (v, v2) = if n < 18 {
                (local_vec3(rng), local_vec3(rng))
            } else {
                (random_vec3(rng), random_vec3(rng))
            };
            // Rays that cross the box: aim from a point outside to a point inside.
            let (v, v2) = if name.starts_with("clip") && n % 3 != 0 {
                let center = [
                    (b[0] + b[3]) / 2.0,
                    (b[1] + b[4]) / 2.0,
                    (b[2] + b[5]) / 2.0,
                ];
                let from = [
                    center[0] - 2.0 + unit(rng),
                    center[1] + unit(rng) - 0.5,
                    center[2] + unit(rng) - 0.5,
                ];
                (from, center)
            } else {
                (v, v2)
            };
            fields.insert("v".into(), vec3_value(v));
            fields.insert("v2".into(), vec3_value(v2));
            let d = if n < 3 { random_vec3(rng) } else { local_vec3(rng) };
            fields.insert("d".into(), vec3_value(d));
            let s = if n < 3 { pick(rng, &SPECIAL) } else { unit(rng) * 2.0 - 0.5 };
            fields.insert("s".into(), h(s));
            fields.insert(
                "pos".into(),
                json!([
                    rng.below(33) as i64 - 16,
                    rng.below(33) as i64 - 16,
                    rng.below(33) as i64 - 16
                ]),
            );
            let count = 1 + rng.below(4) as usize;
            let boxes: Vec<Value> = (0..count).map(|_| box_value(local_box(rng))).collect();
            fields.insert("boxes".into(), Value::Array(boxes));
            out.push(format!("aabb.{name}.{n}"), Value::Object(fields));
        }
    }
}

fn spec_box(b: [f64; 6]) -> Value {
    json!({"t": "box", "b": box_value(b)})
}

fn spec_create(b: [f64; 6]) -> Value {
    json!({"t": "create", "b": box_value(b)})
}

fn spec_or(first: Value, rest: Vec<Value>) -> Value {
    json!({"t": "or", "a": first, "rest": rest})
}

fn spec_join(op: &str, a: &Value, b: &Value, opt: bool) -> Value {
    json!({"t": "join", "op": op, "a": a, "b": b, "opt": opt})
}

fn sixteenths(b: [u32; 6]) -> [f64; 6] {
    [
        f64::from(b[0]) / 16.0,
        f64::from(b[1]) / 16.0,
        f64::from(b[2]) / 16.0,
        f64::from(b[3]) / 16.0,
        f64::from(b[4]) / 16.0,
        f64::from(b[5]) / 16.0,
    ]
}

/// The named shape catalog: vanilla-like geometry, grid-misaligned boxes,
/// degenerate and out-of-range boxes, and seeded random boxes.
fn catalog(rng: &mut Rng) -> Vec<(String, Value)> {
    let mut shapes: Vec<(String, Value)> = vec![
        ("block".into(), json!({"t": "block"})),
        ("empty".into(), json!({"t": "empty"})),
        ("infinity".into(), json!({"t": "infinity"})),
        (
            "half-x".into(),
            spec_box([0.0, 0.0, 0.0, 0.5, 1.0, 1.0]),
        ),
        (
            "slab-bottom".into(),
            spec_box([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]),
        ),
        ("slab-top".into(), spec_box([0.0, 0.5, 0.0, 1.0, 1.0, 1.0])),
        ("plate".into(), spec_box(sixteenths([0, 0, 0, 16, 1, 16]))),
        ("post".into(), spec_box(sixteenths([6, 0, 6, 10, 16, 10]))),
        (
            "stairs".into(),
            spec_or(
                spec_box([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]),
                vec![spec_box([0.0, 0.5, 0.0, 0.5, 1.0, 1.0])],
            ),
        ),
        (
            "fence".into(),
            spec_or(
                spec_box(sixteenths([6, 0, 6, 10, 24, 10])),
                vec![
                    spec_box(sixteenths([7, 6, 0, 9, 9, 6])),
                    spec_box(sixteenths([7, 12, 0, 9, 15, 6])),
                ],
            ),
        ),
        (
            "off-grid".into(),
            spec_box([0.1, 0.2, 0.3, 0.7, 0.8, 0.9]),
        ),
        (
            "near-full".into(),
            spec_box([0.0, 0.0, 0.0, 0.99999995, 1.0, 1.0]),
        ),
        (
            "oversize".into(),
            spec_create([-0.5, -0.5, -0.5, 1.5, 1.5, 1.5]),
        ),
        (
            "eighths".into(),
            spec_box([0.125, 0.125, 0.125, 0.875, 0.875, 0.875]),
        ),
        (
            "degenerate".into(),
            spec_create([0.0, 0.0, 0.0, 1.0E-8, 1.0, 1.0]),
        ),
        (
            "reversed".into(),
            spec_box([1.0, 0.0, 0.0, 0.0, 1.0, 1.0]),
        ),
        (
            "nan".into(),
            spec_create([f64::NAN, 0.0, 0.0, 1.0, 1.0, 1.0]),
        ),
        (
            "disjoint".into(),
            spec_or(
                spec_box([0.0, 0.0, 0.0, 0.25, 0.25, 0.25]),
                vec![spec_box([0.75, 0.75, 0.75, 1.0, 1.0, 1.0])],
            ),
        ),
        (
            "cauldron".into(),
            spec_join(
                "ONLY_FIRST",
                &json!({"t": "block"}),
                &spec_box([0.125, 0.25, 0.125, 0.875, 1.0, 0.875]),
                true,
            ),
        ),
        (
            "ring".into(),
            spec_join(
                "ONLY_FIRST",
                &spec_box([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]),
                &spec_box([0.25, 0.0, 0.25, 0.75, 0.5, 0.75]),
                false,
            ),
        ),
        (
            "moved-post".into(),
            json!({"t": "move", "s": spec_box(sixteenths([6, 0, 6, 10, 16, 10])), "d": vec3_value([0.25, -0.5, 1.0])}),
        ),
        (
            "moved-block".into(),
            json!({"t": "move", "s": {"t": "block"}, "d": vec3_value([0.5, 0.0, 0.0])}),
        ),
        (
            "optimized-stairs".into(),
            json!({"t": "optimize", "s": spec_join(
                "OR",
                &spec_box([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]),
                &spec_box([0.0, 0.5, 0.0, 0.5, 1.0, 1.0]),
                false,
            )}),
        ),
        (
            "single-encompassing".into(),
            json!({"t": "single", "s": spec_or(
                spec_box([0.0, 0.0, 0.0, 0.25, 0.25, 0.25]),
                vec![spec_box([0.75, 0.75, 0.75, 1.0, 1.0, 1.0])],
            )}),
        ),
    ];
    // Seeded boxes on the 1/16 grid (the vanilla case), the 1/8 grid, and off
    // the grid; some or-ed in twos and threes.
    for n in 0..16 {
        let mut b = [0.0; 6];
        for axis in 0..3 {
            let (lo, hi) = {
                let x = rng.below(17) as u32;
                let y = rng.below(17) as u32;
                (x.min(y), x.max(y).max(x.min(y) + 1).min(16))
            };
            b[axis] = f64::from(lo) / 16.0;
            b[axis + 3] = f64::from(hi.max(lo)) / 16.0;
        }
        shapes.push((format!("grid16.{n}"), spec_box(b)));
    }
    for n in 0..6 {
        let b = {
            let mut b = local_box(rng);
            for axis in 0..3 {
                if b[axis] > b[axis + 3] {
                    b.swap(axis, axis + 3);
                }
            }
            b
        };
        shapes.push((format!("local.{n}"), spec_create(b)));
    }
    for n in 0..8 {
        let count = 2 + rng.below(3) as usize;
        let mut parts: Vec<Value> = Vec::new();
        for _ in 0..count {
            let mut b = [0.0; 6];
            for axis in 0..3 {
                let lo = rng.below(16) as u32;
                let hi = lo + 1 + rng.below(u64::from(16 - lo)) as u32;
                b[axis] = f64::from(lo) / 16.0;
                b[axis + 3] = f64::from(hi) / 16.0;
            }
            parts.push(spec_box(b));
        }
        let first = parts.remove(0);
        shapes.push((format!("multi.{n}"), spec_or(first, parts)));
    }
    shapes
}

fn query_none() -> Value {
    json!({"t": "none"})
}

fn shape_request(spec: &Value, query: Value) -> Value {
    json!({"kind": "shape", "spec": spec, "query": query})
}

fn shape_queries(rng: &mut Rng, id: &str, spec: &Value, out: &mut Lines) {
    out.push(format!("{id}.summary"), shape_request(spec, query_none()));
    for axis in AXES {
        for n in 0..2 {
            let source = local_box(rng);
            let source = [
                source[0].min(source[3]),
                source[1].min(source[4]),
                source[2].min(source[5]),
                source[0].max(source[3]),
                source[1].max(source[4]),
                source[2].max(source[5]),
            ];
            let movement = match n {
                0 => unit(rng) * 2.0 - 1.0,
                _ => pick(rng, &[0.0, 1.0E-8, -1.0E-7, 0.5, -0.5, 2.0, -2.0, 1.0E-6]),
            };
            out.push(
                format!("{id}.collide.{axis}.{n}"),
                shape_request(
                    spec,
                    json!({"t": "collide", "axis": axis, "box": box_value(source), "move": h(movement)}),
                ),
            );
        }
    }
    for n in 0..3 {
        let from = [local(rng) - 1.0, local(rng), local(rng)];
        let to = [local(rng) + 1.0, local(rng), local(rng)];
        let pos = [
            rng.below(5) as i64 - 2,
            rng.below(5) as i64 - 2,
            rng.below(5) as i64 - 2,
        ];
        out.push(
            format!("{id}.clip.{n}"),
            shape_request(
                spec,
                json!({"t": "clip", "from": vec3_value(from), "to": vec3_value(to), "pos": pos}),
            ),
        );
    }
    out.push(
        format!("{id}.closest"),
        shape_request(
            spec,
            json!({"t": "closest", "p": vec3_value(local_vec3(rng))}),
        ),
    );
    for axis in AXES {
        out.push(
            format!("{id}.minmax.{axis}"),
            shape_request(
                spec,
                json!({"t": "minmax", "axis": axis, "b": h(local(rng)), "c": h(local(rng))}),
            ),
        );
    }
}

fn shape_corpus(rng: &mut Rng, out: &mut Lines) {
    let shapes = catalog(rng);
    for (name, spec) in &shapes {
        shape_queries(rng, &format!("shape.{name}"), spec, out);
        // The derived shapes: faces (both caches), optimize, move.
        for direction in DIRECTIONS {
            for kind in ["face", "clamped"] {
                out.push(
                    format!("shape.{name}.{kind}.{direction}"),
                    shape_request(
                        &json!({"t": kind, "s": spec, "dir": direction}),
                        query_none(),
                    ),
                );
            }
        }
        out.push(
            format!("shape.{name}.optimize"),
            shape_request(&json!({"t": "optimize", "s": spec}), query_none()),
        );
        out.push(
            format!("shape.{name}.move"),
            shape_request(
                &json!({"t": "move", "s": spec, "d": vec3_value(local_vec3(rng))}),
                query_none(),
            ),
        );
    }
    // Pairs: every BooleanOp, optimised and not, plus the join/equal/occlusion
    // queries.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for _ in 0..70 {
        pairs.push((
            rng.below(shapes.len() as u64) as usize,
            rng.below(shapes.len() as u64) as usize,
        ));
    }
    // The aliasing cases: a shape against itself, and against block/empty.
    for index in 0..shapes.len().min(12) {
        pairs.push((index, index));
        pairs.push((index, 0));
        pairs.push((1, index));
    }
    for (n, (i, j)) in pairs.into_iter().enumerate() {
        let (a_name, a) = &shapes[i];
        let (b_name, b) = &shapes[j];
        for op in BOOLEAN_OPS {
            for opt in [true, false] {
                out.push(
                    format!("pair.{n}.{a_name}.{b_name}.join.{op}.{}", if opt { "opt" } else { "raw" }),
                    shape_request(&spec_join(op, a, b, opt), query_none()),
                );
            }
            out.push(
                format!("pair.{n}.{a_name}.{b_name}.joinq.{op}"),
                shape_request(a, json!({"t": "join", "op": op, "spec2": b})),
            );
        }
        out.push(
            format!("pair.{n}.{a_name}.{b_name}.equal"),
            shape_request(a, json!({"t": "equal", "spec2": b})),
        );
        for direction in DIRECTIONS {
            out.push(
                format!("pair.{n}.{a_name}.{b_name}.occludes.{direction}"),
                shape_request(a, json!({"t": "occludes", "spec2": b, "dir": direction})),
            );
        }
    }
    // Shapes.or over many shapes (the in-place pairwise reduction).
    for n in 0..10 {
        let count = 1 + rng.below(6) as usize;
        let mut parts: Vec<Value> = (0..count)
            .map(|_| shapes[rng.below(shapes.len() as u64) as usize].1.clone())
            .collect();
        let first = parts.remove(0);
        out.push(
            format!("or.{n}"),
            shape_request(&spec_or(first, parts), query_none()),
        );
    }
}

/// `corpus-src/vanilla_shapes.jsonl`, written by `run.sh --dump-vanilla-shapes`.
fn vanilla_path() -> std::path::PathBuf {
    ops_dir().join("corpus-src/vanilla_shapes.jsonl")
}

fn vanilla_corpus(rng: &mut Rng, out: &mut Lines) -> Result<(), String> {
    let text = std::fs::read_to_string(vanilla_path())
        .map_err(|e| format!("cannot read the vanilla shape list: {e}"))?;
    for (index, raw) in text.lines().enumerate() {
        let value: Value = serde_json::from_str(raw)
            .map_err(|e| format!("vanilla_shapes.jsonl:{}: {e}", index + 1))?;
        let boxes = value
            .get("boxes")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("vanilla_shapes.jsonl:{}: no boxes", index + 1))?;
        let creates: Vec<Value> = boxes
            .iter()
            .map(|b| json!({"t": "create", "b": b}))
            .collect();
        let spec = match creates.split_first() {
            None => json!({"t": "empty"}),
            Some((first, rest)) => spec_or(first.clone(), rest.to_vec()),
        };
        let id = format!("vanilla.{index:04}");
        out.push(format!("{id}.summary"), shape_request(&spec, query_none()));
        for axis in AXES {
            let source = [0.3, 0.0, 0.3, 0.7, 1.8, 0.7];
            let shifted = match axis {
                "X" => [-0.4, 0.2, 0.2, 0.2, 0.9, 0.8],
                "Z" => [0.2, 0.2, -0.4, 0.8, 0.9, 0.2],
                _ => source,
            };
            out.push(
                format!("{id}.collide.{axis}"),
                shape_request(
                    &spec,
                    json!({"t": "collide", "axis": axis, "box": box_value(shifted),
                        "move": h(if axis == "Y" { -1.2 } else { 0.9 })}),
                ),
            );
        }
        let from = [local(rng) - 1.0, 0.5 + unit(rng), local(rng)];
        out.push(
            format!("{id}.clip"),
            shape_request(
                &spec,
                json!({"t": "clip", "from": vec3_value(from), "to": vec3_value([0.5, 0.5, 0.5]), "pos": [0, 0, 0]}),
            ),
        );
        out.push(
            format!("{id}.occludes"),
            shape_request(
                &spec,
                json!({"t": "occludes", "spec2": {"t": "block"}, "dir": DIRECTIONS[index % 6]}),
            ),
        );
    }
    Ok(())
}

/// Build the corpus text.
pub fn emit() -> Result<String, String> {
    let mut out = Lines(String::new());
    vec3_corpus(&mut Rng::for_stream("shapes.vec3"), &mut out);
    aabb_corpus(&mut Rng::for_stream("shapes.aabb"), &mut out);
    shape_corpus(&mut Rng::for_stream("shapes.shape"), &mut out);
    vanilla_corpus(&mut Rng::for_stream("shapes.vanilla"), &mut out)?;
    Ok(out.0)
}
