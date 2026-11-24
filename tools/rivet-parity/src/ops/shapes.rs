//! `shapes.eval`: the Rust twin of the reference oracle's op of that name.
//!
//! The op runs `net.minecraft.world.phys` (`Vec3`, `AABB`) and
//! `net.minecraft.world.phys.shapes` (`Shapes`, `VoxelShape`, `BooleanOp`, with
//! Paper's Moonrise collision patches) on one request. A request names a
//! `kind`:
//!
//! - `vec3`  `{fn, a, b, s, f, g, t}` — one `Vec3` function;
//! - `aabb`  `{fn, box, box2, v, v2, d, s, boxes, pos}` — one `AABB` function;
//! - `shape` `{spec, query}` — build a shape from a `spec` tree and answer its
//!   summary plus one `query`.
//!
//! Every double and float travels as IEEE-754 bit hex (`0x...`, NaN
//! canonicalised as D14 says), so requests and answers are exact. The answer
//! is `{"out": ...}`; the key order inside `out` is the order the Java op
//! inserts, and the call order below mirrors Java's call order (the Moonrise
//! caches `VoxelShape` fills make some answers depend on it).
//!
//! `IllegalArgumentException` (`Shapes.box` with a min above its max) and
//! `UnsupportedOperationException` (a join operator with `(false, false) ->
//! true`) are the Java op's input-rejection verdicts; a panic in the ported
//! code is their Rust spelling.

use rivet_registry::core::{Axis, BlockPos, Direction};
use rivet_registry::world::phys::{AABB, BlockHitResult, HitResultBehavior, Vec3};
use rivet_world::phys::shapes::boolean_op::BooleanOp;
use rivet_world::phys::shapes::shapes;
use rivet_world::phys::shapes::voxel_shape::VoxelShape;
use serde_json::{Map, Value, json};

use super::compare::Outcome;
use super::packet;

/// `Double.doubleToLongBits` as `0x` + 16 hex digits (NaN canonical).
pub fn f64_bits(value: f64) -> String {
    let bits = if value.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        value.to_bits()
    };
    format!("0x{bits:016x}")
}

/// `Float.floatToIntBits` as `0x` + 8 hex digits (NaN canonical).
pub fn f32_bits(value: f32) -> String {
    let bits = if value.is_nan() {
        0x7fc0_0000
    } else {
        value.to_bits()
    };
    format!("0x{bits:08x}")
}

fn parse_f64(value: &Value) -> Result<f64, String> {
    let text = value.as_str().ok_or("f64 bits must be a string")?;
    let hex = text.strip_prefix("0x").ok_or("f64 bits need 0x")?;
    if hex.len() != 16 {
        return Err(format!("bad f64 bits {text}"));
    }
    u64::from_str_radix(hex, 16)
        .map(f64::from_bits)
        .map_err(|e| e.to_string())
}

fn parse_f32(value: &Value) -> Result<f32, String> {
    let text = value.as_str().ok_or("f32 bits must be a string")?;
    let hex = text.strip_prefix("0x").ok_or("f32 bits need 0x")?;
    if hex.len() != 8 {
        return Err(format!("bad f32 bits {text}"));
    }
    u32::from_str_radix(hex, 16)
        .map(f32::from_bits)
        .map_err(|e| e.to_string())
}

fn array(value: &Value, len: usize) -> Result<&Vec<Value>, String> {
    match value.as_array() {
        Some(items) if items.len() == len => Ok(items),
        _ => Err(format!("expected an array of {len}")),
    }
}

fn parse_vec3(value: &Value) -> Result<Vec3, String> {
    let a = array(value, 3)?;
    Ok(Vec3::new(
        parse_f64(&a[0])?,
        parse_f64(&a[1])?,
        parse_f64(&a[2])?,
    ))
}

fn parse_aabb(value: &Value) -> Result<AABB, String> {
    let a = array(value, 6)?;
    Ok(AABB::new(
        parse_f64(&a[0])?,
        parse_f64(&a[1])?,
        parse_f64(&a[2])?,
        parse_f64(&a[3])?,
        parse_f64(&a[4])?,
        parse_f64(&a[5])?,
    ))
}

fn parse_pos(value: &Value) -> Result<BlockPos, String> {
    let a = array(value, 3)?;
    let int = |v: &Value| {
        v.as_i64()
            .and_then(|i| i32::try_from(i).ok())
            .ok_or_else(|| "block pos components must be i32".to_owned())
    };
    Ok(BlockPos::new(int(&a[0])?, int(&a[1])?, int(&a[2])?))
}

fn parse_direction(value: &Value) -> Result<Direction, String> {
    match value.as_str() {
        Some("DOWN") => Ok(Direction::Down),
        Some("UP") => Ok(Direction::Up),
        Some("NORTH") => Ok(Direction::North),
        Some("SOUTH") => Ok(Direction::South),
        Some("WEST") => Ok(Direction::West),
        Some("EAST") => Ok(Direction::East),
        other => Err(format!("unknown direction {other:?}")),
    }
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "DOWN",
        Direction::Up => "UP",
        Direction::North => "NORTH",
        Direction::South => "SOUTH",
        Direction::West => "WEST",
        Direction::East => "EAST",
    }
}

fn parse_axis(value: &Value) -> Result<Axis, String> {
    match value.as_str() {
        Some("X") => Ok(Axis::X),
        Some("Y") => Ok(Axis::Y),
        Some("Z") => Ok(Axis::Z),
        other => Err(format!("unknown axis {other:?}")),
    }
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}

/// `BooleanOp.<NAME>`, by Java field name.
pub fn boolean_op(name: &str) -> Result<BooleanOp, String> {
    Ok(match name {
        "FALSE" => BooleanOp::FALSE,
        "NOT_OR" => BooleanOp::NOT_OR,
        "ONLY_SECOND" => BooleanOp::ONLY_SECOND,
        "NOT_FIRST" => BooleanOp::NOT_FIRST,
        "ONLY_FIRST" => BooleanOp::ONLY_FIRST,
        "NOT_SECOND" => BooleanOp::NOT_SECOND,
        "NOT_SAME" => BooleanOp::NOT_SAME,
        "NOT_AND" => BooleanOp::NOT_AND,
        "AND" => BooleanOp::AND,
        "SAME" => BooleanOp::SAME,
        "SECOND" => BooleanOp::SECOND,
        "CAUSES" => BooleanOp::CAUSES,
        "FIRST" => BooleanOp::FIRST,
        "CAUSED_BY" => BooleanOp::CAUSED_BY,
        "OR" => BooleanOp::OR,
        "TRUE" => BooleanOp::TRUE,
        other => return Err(format!("unknown BooleanOp {other}")),
    })
}

fn bits_of(values: &[f64]) -> Value {
    Value::Array(values.iter().map(|v| json!(f64_bits(*v))).collect())
}

fn vec3_bits(v: &Vec3) -> Value {
    bits_of(&[v.x, v.y, v.z])
}

fn aabb_bits(b: &AABB) -> Value {
    bits_of(&[b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z])
}

fn field<'a>(request: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    request.get(key).ok_or_else(|| format!("missing {key}"))
}

fn text<'a>(request: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    field(request, key)?
        .as_str()
        .ok_or_else(|| format!("{key} must be a string"))
}

fn opt<T>(
    request: &Map<String, Value>,
    key: &str,
    parse: impl Fn(&Value) -> Result<T, String>,
) -> Result<Option<T>, String> {
    request.get(key).map(parse).transpose()
}

/// `shapes.eval`.
pub fn shapes_eval(request: &Map<String, Value>) -> Outcome {
    let kind = match text(request, "kind") {
        Ok(kind) => kind,
        Err(message) => return Outcome::Unavailable(message),
    };
    let evaluated = packet::guarded(|| match kind {
        "vec3" => vec3(request),
        "aabb" => aabb(request),
        "shape" => shape(request),
        other => Err(format!("unknown shapes.eval kind {other}")),
    });
    match evaluated {
        Err(_) => Outcome::Rejected,
        Ok(Err(message)) => Outcome::Unavailable(message),
        Ok(Ok(out)) => {
            let mut result = Map::new();
            result.insert("out".into(), Value::Object(out));
            Outcome::Accepted(result)
        }
    }
}

/// The per-subject tally label of one corpus input.
pub fn subject_of(request: &Map<String, Value>) -> Option<String> {
    let kind = request.get("kind")?.as_str()?;
    match kind {
        "vec3" | "aabb" => Some(format!("{kind}.{}", request.get("fn")?.as_str()?)),
        "shape" => Some(format!(
            "shape.{}",
            request.get("query")?.get("t")?.as_str()?
        )),
        _ => None,
    }
}

fn single(key: &str, value: Value) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert(key.into(), value);
    out
}

fn vec3(request: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let name = text(request, "fn")?;
    let a = opt(request, "a", parse_vec3)?;
    let b = opt(request, "b", parse_vec3)?;
    let s = opt(request, "s", parse_f64)?.unwrap_or(0.0);
    let f = opt(request, "f", parse_f32)?.unwrap_or(0.0);
    let g = opt(request, "g", parse_f32)?.unwrap_or(0.0);
    let need_a = || a.ok_or_else(|| "missing a".to_owned());
    let need_b = || b.ok_or_else(|| "missing b".to_owned());
    let v = |x: Vec3| single("v", vec3_bits(&x));
    let n = |x: f64| single("n", json!(f64_bits(x)));
    Ok(match name {
        "add" => v(need_a()?.add(&need_b()?)),
        "addScalar" => v(need_a()?.add_value(s)),
        "subtract" => v(need_a()?.subtract(&need_b()?)),
        "subtractScalar" => v(need_a()?.subtract_value(s)),
        "multiply" => v(need_a()?.multiply(&need_b()?)),
        "scale" => v(need_a()?.scale(s)),
        "cross" => v(need_a()?.cross(&need_b()?)),
        "vectorTo" => v(need_a()?.vector_to(&need_b()?)),
        "normalize" => v(need_a()?.normalize()),
        "reverse" => v(need_a()?.reverse()),
        "horizontal" => v(need_a()?.horizontal()),
        "lerp" => v(need_a()?.lerp(&need_b()?, s)),
        "xRot" => v(need_a()?.x_rot(f)),
        "yRot" => v(need_a()?.y_rot(f)),
        "zRot" => v(need_a()?.z_rot(f)),
        "rotateClockwise90" => v(need_a()?.rotate_clockwise90()),
        "projectedOn" => v(need_a()?.projected_on(&need_b()?)),
        "addLocalCoordinates" => v(need_a()?.add_local_coordinates(&need_b()?)),
        "directionFromRotation" => v(Vec3::direction_from_rotation_xy(f, g)),
        "dot" => n(need_a()?.dot(&need_b()?)),
        "length" => n(need_a()?.length()),
        "lengthSqr" => n(need_a()?.length_sqr()),
        "distanceTo" => n(need_a()?.distance_to(&need_b()?)),
        "distanceToSqr" => n(need_a()?.distance_to_sqr(&need_b()?)),
        "horizontalDistance" => n(need_a()?.horizontal_distance()),
        "horizontalDistanceSqr" => n(need_a()?.horizontal_distance_sqr()),
        "closerThan" => single("b", json!(need_a()?.closer_than(&need_b()?, s))),
        "closerThanXzY" => {
            let t = parse_f64(field(request, "t")?)?;
            single("b", json!(need_a()?.closer_than_xz_y(&need_b()?, s, t)))
        }
        "isFinite" => single("b", json!(need_a()?.is_finite())),
        "equals" => single("b", json!(need_a()? == need_b()?)),
        "hashCode" => single("i", json!(need_a()?.hash_code())),
        "toString" => single("s", json!(need_a()?.to_string())),
        "rotation" => {
            let rotation = need_a()?.rotation();
            single("f", json!([f32_bits(rotation.x), f32_bits(rotation.y)]))
        }
        other => return Err(format!("unknown vec3 fn {other}")),
    })
}

fn boxes_of(request: &Map<String, Value>) -> Result<Vec<AABB>, String> {
    field(request, "boxes")?
        .as_array()
        .ok_or("boxes must be an array")?
        .iter()
        .map(parse_aabb)
        .collect()
}

fn block_hit(out: &mut Map<String, Value>, hit: Option<BlockHitResult>) {
    // The recorded canonical form drops absent (null) results, so a miss leaves the key out.
    if let Some(hit) = hit {
        let pos = hit.get_block_pos();
        out.insert(
            "hit".into(),
            json!({
                "loc": vec3_bits(&hit.get_location()),
                "dir": direction_name(hit.get_direction()),
                "inside": hit.is_inside(),
                "pos": [pos.get_x(), pos.get_y(), pos.get_z()],
            }),
        );
    }
}

fn aabb(request: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let name = text(request, "fn")?;
    let bx = parse_aabb(field(request, "box")?)?;
    let box2 = opt(request, "box2", parse_aabb)?;
    let v = opt(request, "v", parse_vec3)?;
    let v2 = opt(request, "v2", parse_vec3)?;
    let d = opt(request, "d", parse_vec3)?;
    let s = opt(request, "s", parse_f64)?.unwrap_or(0.0);
    let need_box2 = || box2.ok_or_else(|| "missing box2".to_owned());
    let need_v = || v.ok_or_else(|| "missing v".to_owned());
    let need_v2 = || v2.ok_or_else(|| "missing v2".to_owned());
    let need_d = || d.ok_or_else(|| "missing d".to_owned());
    let out_box = |b: AABB| single("box", aabb_bits(&b));
    let n = |x: f64| single("n", json!(f64_bits(x)));
    let flag = |x: bool| single("b", json!(x));
    let point = |p: Vec3| single("v", vec3_bits(&p));
    Ok(match name {
        "new" => out_box(bx),
        "move" => {
            let d = need_d()?;
            out_box(bx.move_xyz(d.x, d.y, d.z))
        }
        "moveVec" => out_box(bx.move_vec3(&need_d()?)),
        "moveBlockPos" => out_box(bx.move_block_pos(&parse_pos(field(request, "pos")?)?)),
        "inflate3" => {
            let d = need_d()?;
            out_box(bx.inflate_xyz(d.x, d.y, d.z))
        }
        "inflate" => out_box(bx.inflate(s)),
        "deflate3" => {
            let d = need_d()?;
            out_box(bx.deflate_xyz(d.x, d.y, d.z))
        }
        "deflate" => out_box(bx.deflate(s)),
        "contract" => {
            let d = need_d()?;
            out_box(bx.contract(d.x, d.y, d.z))
        }
        "expandTowards" => out_box(bx.expand_towards(&need_d()?)),
        "expandTowards3" => {
            let d = need_d()?;
            out_box(bx.expand_towards_xyz(d.x, d.y, d.z))
        }
        "intersect" => out_box(bx.intersect(&need_box2()?)),
        "minmax" => out_box(bx.minmax(&need_box2()?)),
        "setMinX" => out_box(bx.set_min_x(s)),
        "setMaxY" => out_box(bx.set_max_y(s)),
        "setMinZ" => out_box(bx.set_min_z(s)),
        "intersects" => flag(bx.intersects(&need_box2()?)),
        "intersectsVecs" => flag(bx.intersects_vec3s(&need_v()?, &need_v2()?)),
        "intersectsBlockPos" => flag(bx.intersects_block_pos(&parse_pos(field(request, "pos")?)?)),
        "contains" => flag(bx.contains(&need_v()?)),
        "hasNaN" => flag(bx.has_nan()),
        "equals" => flag(bx == need_box2()?),
        "hashCode" => single("i", json!(bx.hash_code())),
        "toString" => single("s", json!(bx.to_string())),
        "getSize" => n(bx.get_size()),
        "getXsize" => n(bx.get_xsize()),
        "getYsize" => n(bx.get_ysize()),
        "getZsize" => n(bx.get_zsize()),
        "distanceToSqrVec" => n(bx.distance_to_sqr(&need_v()?)),
        "distanceToSqrBox" => n(bx.distance_to_sqr_aabb(&need_box2()?)),
        "getCenter" => point(bx.get_center()),
        "getBottomCenter" => point(bx.get_bottom_center()),
        "getMinPosition" => point(bx.get_min_position()),
        "getMaxPosition" => point(bx.get_max_position()),
        "clip" => {
            let hit = bx.clip(&need_v()?, &need_v2()?);
            let mut out = Map::new();
            if let Some(p) = hit {
                out.insert("hit".into(), vec3_bits(&p));
            }
            out
        }
        "clipAll" => {
            let mut out = Map::new();
            block_hit(
                &mut out,
                AABB::clip_all(
                    &boxes_of(request)?,
                    &need_v()?,
                    &need_v2()?,
                    &parse_pos(field(request, "pos")?)?,
                ),
            );
            out
        }
        "collidedAlongVector" => flag(bx.collided_along_vector(&need_v()?, &boxes_of(request)?)),
        "ofSize" => {
            let d = need_d()?;
            out_box(AABB::of_size(&need_v()?, d.x, d.y, d.z))
        }
        other => return Err(format!("unknown aabb fn {other}")),
    })
}

fn build_shape(spec: &Value) -> Result<VoxelShape, String> {
    let spec = spec.as_object().ok_or("shape spec must be an object")?;
    let b6 = || -> Result<[f64; 6], String> {
        let a = array(field(spec, "b")?, 6)?;
        let mut out = [0.0; 6];
        for (slot, value) in out.iter_mut().zip(a) {
            *slot = parse_f64(value)?;
        }
        Ok(out)
    };
    Ok(match text(spec, "t")? {
        "box" => {
            let b = b6()?;
            shapes::box_(b[0], b[1], b[2], b[3], b[4], b[5])
        }
        "create" => {
            let b = b6()?;
            shapes::create(b[0], b[1], b[2], b[3], b[4], b[5])
        }
        "block" => shapes::block(),
        "empty" => shapes::empty(),
        "infinity" => shapes::INFINITY.clone(),
        "join" => {
            let a = build_shape(field(spec, "a")?)?;
            let b = build_shape(field(spec, "b")?)?;
            let op = boolean_op(text(spec, "op")?)?;
            if field(spec, "opt")?.as_bool().ok_or("opt must be a bool")? {
                shapes::join(&a, &b, op)
            } else {
                shapes::join_unoptimized(&a, &b, op)
            }
        }
        "or" => {
            let first = build_shape(field(spec, "a")?)?;
            let rest: Vec<VoxelShape> = field(spec, "rest")?
                .as_array()
                .ok_or("rest must be an array")?
                .iter()
                .map(build_shape)
                .collect::<Result<_, _>>()?;
            shapes::or_many(&first, &rest)
        }
        "move" => {
            let d = parse_vec3(field(spec, "d")?)?;
            build_shape(field(spec, "s")?)?.move_xyz(d.x, d.y, d.z)
        }
        "optimize" => build_shape(field(spec, "s")?)?.optimize(),
        "face" => {
            build_shape(field(spec, "s")?)?.get_face_shape(parse_direction(field(spec, "dir")?)?)
        }
        "clamped" => build_shape(field(spec, "s")?)?
            .moonrise_get_face_shape_clamped(parse_direction(field(spec, "dir")?)?),
        "single" => build_shape(field(spec, "s")?)?.single_encompassing(),
        other => return Err(format!("unknown shape spec {other}")),
    })
}

fn shape_lite(shape: &VoxelShape) -> Value {
    let mut lite = Map::new();
    lite.insert("empty".into(), json!(shape.is_empty()));
    lite.insert("full".into(), json!(shape.moonrise_is_full_block()));
    lite.insert(
        "occludes".into(),
        json!(shape.moonrise_occludes_full_block()),
    );
    let aabbs: Vec<Value> = shape.to_aabbs().iter().map(aabb_bits).collect();
    lite.insert("aabbs".into(), Value::Array(aabbs));
    Value::Object(lite)
}

fn shape_summary(shape: &VoxelShape) -> Value {
    let Value::Object(mut summary) = shape_lite(shape) else {
        unreachable!("shape_lite builds an object")
    };
    if !shape.is_empty() {
        summary.insert("bounds".into(), aabb_bits(&shape.bounds()));
    }
    let grid = shape.shape();
    summary.insert(
        "size".into(),
        json!([grid.get_x_size(), grid.get_y_size(), grid.get_z_size()]),
    );
    let mut coords = Map::new();
    for axis in Axis::VALUES {
        coords.insert(
            axis_name(axis).into(),
            bits_of(&shape.get_coords(axis).to_double_array()),
        );
    }
    summary.insert("coords".into(), Value::Object(coords));
    let mut voxels = String::new();
    for x in 0..grid.get_x_size() {
        for y in 0..grid.get_y_size() {
            for z in 0..grid.get_z_size() {
                voxels.push(if grid.is_full(x, y, z) { '1' } else { '0' });
            }
        }
    }
    summary.insert("voxels".into(), json!(voxels));
    let mut min = Vec::new();
    let mut max = Vec::new();
    for axis in Axis::VALUES {
        min.push(json!(f64_bits(shape.min(axis))));
        max.push(json!(f64_bits(shape.max(axis))));
    }
    summary.insert("min".into(), Value::Array(min));
    summary.insert("max".into(), Value::Array(max));
    let mut faces = Map::new();
    let mut clamped = Map::new();
    for direction in Direction::VALUES {
        faces.insert(
            direction_name(direction).into(),
            shape_lite(&shape.get_face_shape(direction)),
        );
        clamped.insert(
            direction_name(direction).into(),
            shape_lite(&shape.moonrise_get_face_shape_clamped(direction)),
        );
    }
    summary.insert("faces".into(), Value::Object(faces));
    summary.insert("clamped".into(), Value::Object(clamped));
    Value::Object(summary)
}

fn shape(request: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let shape = build_shape(field(request, "spec")?)?;
    let mut out = Map::new();
    out.insert("summary".into(), shape_summary(&shape));
    let query = field(request, "query")?
        .as_object()
        .ok_or("query must be an object")?;
    let mut answer = Map::new();
    match text(query, "t")? {
        "none" => {}
        "collide" => {
            let value = shape.collide(
                parse_axis(field(query, "axis")?)?,
                &parse_aabb(field(query, "box")?)?,
                parse_f64(field(query, "move")?)?,
            );
            answer.insert("v".into(), json!(f64_bits(value)));
        }
        "clip" => block_hit(
            &mut answer,
            shape.clip(
                &parse_vec3(field(query, "from")?)?,
                &parse_vec3(field(query, "to")?)?,
                &parse_pos(field(query, "pos")?)?,
            ),
        ),
        "closest" => {
            let point = shape.closest_point_to(&parse_vec3(field(query, "p")?)?);
            if let Some(p) = point {
                answer.insert("p".into(), vec3_bits(&p));
            }
        }
        "minmax" => {
            let axis = parse_axis(field(query, "axis")?)?;
            let b = parse_f64(field(query, "b")?)?;
            let c = parse_f64(field(query, "c")?)?;
            answer.insert("min".into(), json!(f64_bits(shape.min_bc(axis, b, c))));
            answer.insert("max".into(), json!(f64_bits(shape.max_bc(axis, b, c))));
        }
        "join" => {
            let other = build_shape(field(query, "spec2")?)?;
            let op = boolean_op(text(query, "op")?)?;
            answer.insert(
                "nonEmpty".into(),
                json!(shapes::join_is_not_empty(&shape, &other, op)),
            );
            answer.insert(
                "joined".into(),
                shape_lite(&shapes::join(&shape, &other, op)),
            );
            answer.insert(
                "unoptimized".into(),
                shape_lite(&shapes::join_unoptimized(&shape, &other, op)),
            );
        }
        "equal" => {
            let other = build_shape(field(query, "spec2")?)?;
            answer.insert("equal".into(), json!(shapes::equal(&shape, &other)));
        }
        "occludes" => {
            let other = build_shape(field(query, "spec2")?)?;
            let direction = parse_direction(field(query, "dir")?)?;
            answer.insert(
                "block".into(),
                json!(shapes::block_occludes(&shape, &other, direction)),
            );
            answer.insert(
                "merged".into(),
                json!(shapes::merged_face_occludes(&shape, &other, direction)),
            );
            answer.insert(
                "face".into(),
                json!(shapes::face_shape_occludes(&shape, &other)),
            );
            answer.insert(
                "or".into(),
                shape_lite(&shape.moonrise_or_unoptimized(&other)),
            );
        }
        other => return Err(format!("unknown shape query {other}")),
    }
    out.insert("query".into(), Value::Object(answer));
    Ok(out)
}
