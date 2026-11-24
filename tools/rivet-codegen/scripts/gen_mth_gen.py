#!/usr/bin/env python3
"""Generate src/java/MthGen.java — the Java oracle that recomputes every golden
Mth vector plus the SIN/ASIN_TAB/COS_TAB tables from the real Paper Mth class.

Reads tools/rivet-codegen/data/mth_vectors.tsv (index, rust-lhs, committed-rhs)
and emits Java statements, in placeholder order, that call the exact Mth
overload each Rust fn corresponds to (f32 args are forced to float so Java
resolves the float overload, matching the Rust fn's parameter types).

The generated MthGen prints:
  - SIN_TAB_START ... 65536 lines of `0x%08x` f32 bits
  - ASIN_TAB_START ... 257 lines of `0x%016x` f64 bits
  - COS_TAB_START  ... 257 lines of `0x%016x` f64 bits
  - then one line per golden assertion (index 0..N), formatted exactly as the
    committed Rust literal (0x.. / -123i32 / true / vec![...]).

RNG variables declared in the golden file are re-declared per golden_ section,
so each section is emitted inside its own `{ ... }` Java scope (fresh instances,
matching the per-test `let mut rX = ...`).

Output: tools/rivet-codegen/src/java/MthGen.java (generated; not committed — see
README — but produced on demand by `rivet-codegen mth-gen`).
"""
from __future__ import annotations

import re
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent.parent
VECTORS = Path(__file__).resolve().parent.parent / "data/mth_vectors.tsv"
SKELETON = Path(__file__).resolve().parent.parent / "data/mth_golden_skeleton.rs"
OUT = Path(__file__).resolve().parent.parent / "src/java/MthGen.java"

# fn_name -> (java_call, param_types, value_format)
# value_format: F32HEX | F64HEX | I32 | I64 | I8 | BOOL | VEC
SPEC = {
    "sin": ("Mth.sin", "f64", "F32HEX"),
    "cos": ("Mth.cos", "f64", "F32HEX"),
    "sqrt": ("Mth.sqrt", "f32", "F32HEX"),
    "floor_d": ("Mth.floor", "f64", "I32"),
    "floor": ("Mth.floor", "f32", "I32"),
    "lfloor": ("Mth.lfloor", "f64", "I64"),
    "ceil": ("Mth.ceil", "f32", "I32"),
    "ceil_d": ("Mth.ceil", "f64", "I32"),
    "ceil_long": ("Mth.ceilLong", "f64", "I64"),
    "abs_i32": ("Math.abs", "i32", "I32"),
    "abs": ("Math.abs", "f32", "F32HEX"),
    "abs_max": ("Mth.absMax", "i32,i32", "I32"),
    "chessboard_distance": ("Mth.chessboardDistance", "i32,i32,i32,i32", "I32"),
    "clamp": ("Mth.clamp", None, "CLAMP"),
    "clamp_f32": ("Mth.clamp", "f32,f32,f32", "F32HEX"),
    "clamp_f64": ("Mth.clamp", "f64,f64,f64", "F64HEX"),
    "clamped_lerp": ("Mth.clampedLerp", "f64,f64,f64", "F64HEX"),
    "clamped_lerp_f32": ("Mth.clampedLerp", "f32,f32,f32", "F32HEX"),
    "lerp": ("Mth.lerp", "f64,f64,f64", "F64HEX"),
    "lerp_f32": ("Mth.lerp", "f32,f32,f32", "F32HEX"),
    "lerp_int": ("Mth.lerpInt", "f32,i32,i32", "I32"),
    "lerp_discrete": ("Mth.lerpDiscrete", "f32,i32,i32", "I32"),
    "lerp2": ("Mth.lerp2", "f64,f64,f64,f64,f64,f64", "F64HEX"),
    "lerp3": ("Mth.lerp3", "f64,f64,f64,f64,f64,f64,f64,f64,f64,f64,f64", "F64HEX"),
    "wrap_degrees": ("Mth.wrapDegrees", "i32", "I32"),
    "wrap_degrees_i64": ("Mth.wrapDegrees", "i64", "F32HEX"),
    "wrap_degrees_f32": ("Mth.wrapDegrees", "f32", "F32HEX"),
    "wrap_degrees_f64": ("Mth.wrapDegrees", "f64", "F64HEX"),
    "wrap_degrees90": ("Mth.wrapDegrees90", "f32", "F32HEX"),
    "degrees_difference": ("Mth.degreesDifference", "f32,f32", "F32HEX"),
    "degrees_difference_abs": ("Mth.degreesDifferenceAbs", "f32,f32", "F32HEX"),
    "rotate_if_necessary": ("Mth.rotateIfNecessary", "f32,f32,f32", "F32HEX"),
    "approach": ("Mth.approach", "f32,f32,f32", "F32HEX"),
    "approach_degrees": ("Mth.approachDegrees", "f32,f32,f32", "F32HEX"),
    "get_int": ("Mth.getInt", "optstr,i32", "I32"),
    "smallest_square_side": ("Mth.smallestSquareSide", "i32", "I32"),
    "smallest_encompassing_power_of_two": ("Mth.smallestEncompassingPowerOfTwo", "i32", "I32"),
    "is_power_of_two": ("Mth.isPowerOfTwo", "i32", "BOOL"),
    "is_power_of_two_i64": ("Mth.isPowerOfTwo", "i64", "BOOL"),
    "ceillog2": ("Mth.ceillog2", "i32", "I32"),
    "log2": ("Mth.log2", "i32", "I32"),
    "frac": ("Mth.frac", "f32", "F32HEX"),
    "frac_f64": ("Mth.frac", "f64", "F64HEX"),
    "get_seed": ("Mth.getSeed", "i32,i32,i32", "I64"),
    "murmur_hash3_mixer": ("Mth.murmurHash3Mixer", "i32", "I32"),
    "positive_modulo": ("Math.floorMod", "i32,i32", "I32"),
    "positive_modulo_f32": ("Mth.positiveModulo", "f32,f32", "F32HEX"),
    "positive_modulo_f64": ("Mth.positiveModulo", "f64,f64", "F64HEX"),
    "floor_div": ("Math.floorDiv", "i32,i32", "I32"),
    "is_multiple_of": ("Mth.isMultipleOf", "i32,i32", "BOOL"),
    "pack_degrees": ("Mth.packDegrees", "f32", "I8"),
    "unpack_degrees": ("Mth.unpackDegrees", "i8", "F32HEX"),
    "atan2": ("Mth.atan2", "f64,f64", "F64HEX"),
    "inv_sqrt": ("Mth.invSqrt", "f32", "F32HEX"),
    "inv_sqrt_f64": ("Mth.invSqrt", "f64", "F64HEX"),
    "fast_inv_sqrt": ("Mth.fastInvSqrt", "f64", "F64HEX"),
    "fast_inv_cube_root": ("Mth.fastInvCubeRoot", "f32", "F32HEX"),
    "hsv_to_argb": ("Mth.hsvToArgb", "f32,f32,f32,i32", "I32"),
    "hsv_to_rgb": ("Mth.hsvToRgb", "f32,f32,f32", "I32"),
    "catmullrom": ("Mth.catmullrom", "f32,f32,f32,f32,f32", "F32HEX"),
    "smoothstep": ("Mth.smoothstep", "f64", "F64HEX"),
    "smoothstep_derivative": ("Mth.smoothstepDerivative", "f64", "F64HEX"),
    "sign": ("Mth.sign", "f64", "I32"),
    "rot_lerp": ("Mth.rotLerp", "f32,f32,f32", "F32HEX"),
    "rot_lerp_f64": ("Mth.rotLerp", "f64,f64,f64", "F64HEX"),
    "rot_lerp_rad": ("Mth.rotLerpRad", "f32,f32,f32", "F32HEX"),
    "triangle_wave": ("Mth.triangleWave", "f32,f32", "F32HEX"),
    "square_i32": ("Mth.square", "i32", "I32"),
    "square_i64": ("Mth.square", "i64", "I64"),
    "square_f32": ("Mth.square", "f32", "F32HEX"),
    "cube": ("Mth.cube", "f32", "F32HEX"),
    "square_f64": ("Mth.square", "f64", "F64HEX"),
    "clamped_map": ("Mth.clampedMap", "f64,f64,f64,f64,f64", "F64HEX"),
    "map": ("Mth.map", "f64,f64,f64,f64,f64", "F64HEX"),
    "inverse_lerp": ("Mth.inverseLerp", "f64,f64,f64", "F64HEX"),
    "clamped_map_f32": ("Mth.clampedMap", "f32,f32,f32,f32,f32", "F32HEX"),
    "map_f32": ("Mth.map", "f32,f32,f32,f32,f32", "F32HEX"),
    "length_squared": ("Mth.lengthSquared", "f64,f64", "F64HEX"),
    "length": ("Mth.length", "f64,f64", "F64HEX"),
    "length_f32": ("Mth.length", "f32,f32", "F32HEX"),
    "length_squared_xyz": ("Mth.lengthSquared", "f64,f64,f64", "F64HEX"),
    "length_xyz": ("Mth.length", "f64,f64,f64", "F64HEX"),
    "length_squared_xyz_f32": ("Mth.lengthSquared", "f32,f32,f32", "F32HEX"),
    "quantize": ("Mth.quantize", "f64,i32", "I32"),
    "positive_ceil_div": ("Mth.positiveCeilDiv", "i32,i32", "I32"),
    "round_toward": ("Mth.roundToward", "i32,i32", "I32"),
    "positive_ceil_div_i64": ("Mth.positiveCeilDiv", "i64,i64", "I64"),
    "round_toward_i64": ("Mth.roundToward", "i64,i64", "I64"),
    "wobble": ("Mth.wobble", "f64", "F64HEX"),
    "next_int": ("Mth.nextInt", "rng,i32,i32", "I32"),
    "next_float": ("Mth.nextFloat", "rng,f32,f32", "F32HEX"),
    "next_double": ("Mth.nextDouble", "rng,f64,f64", "F64HEX"),
    "random_between_inclusive": ("Mth.randomBetweenInclusive", "rng,i32,i32", "I32"),
    "random_between": ("Mth.randomBetween", "rng,f32,f32", "F32HEX"),
    "normal": ("Mth.normal", "rng,f32,f32", "F32HEX"),
}

RNG_SPEC = {
    "next_int": ("nextInt", "I32"),
    "next_int_bound": ("nextInt", "I32"),
    "next_float": ("nextFloat", "F32HEX"),
    "next_double": ("nextDouble", "F64HEX"),
    "next_long": ("nextLong", "I64"),
    "next_boolean": ("nextBoolean", "BOOL"),
    "next_gaussian": ("nextGaussian", "F64HEX"),
    "triangle_f64": ("triangle", "F64HEX"),
    "next_int_between_inclusive": ("nextIntBetweenInclusive", "I32"),
}

HEADER = """\
// GENERATED by tools/rivet-codegen/scripts/gen_mth_gen.py from the committed
// mth_golden_tests.rs assertions + the real Paper Mth class. Do not hand-edit.
import java.lang.reflect.Field;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.SingleThreadedRandomSource;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.AABB;
import org.apache.commons.lang3.math.Fraction;
import org.joml.Quaternionf;
import org.joml.Vector3f;

public class MthGen {
    // floatToIntBits/doubleToLongBits (not the Raw variants): hardware-created
    // NaN sign/payload is architecture-undefined (D14), so the oracle emits the
    // canonical NaN bit-pattern; every non-NaN value is bit-exact and identical
    // to the Raw forms.
    static void pF(float v) { System.out.printf("0x%08x%n", Float.floatToIntBits(v)); }
    static void pD(double v) { System.out.printf("0x%016x%n", Double.doubleToLongBits(v)); }
    static void pI(int v) { System.out.println(v + "i32"); }
    static void pL(long v) { System.out.println(v + "i64"); }
    static void pB(byte v) { System.out.println(v); }
    static void pBool(boolean v) { System.out.println(v); }
    static void pVec(int[] a) {
        StringBuilder sb = new StringBuilder("vec![");
        for (int i = 0; i < a.length; i++) { if (i > 0) sb.append(", "); sb.append(a[i]); }
        sb.append("]");
        System.out.println(sb);
    }

    static void tables() throws Exception {
        Class<?> mth = Class.forName("net.minecraft.util.Mth");
        Field sinF = mth.getDeclaredField("SIN"); sinF.setAccessible(true);
        float[] sin = (float[]) sinF.get(null);
        Field asinF = mth.getDeclaredField("ASIN_TAB"); asinF.setAccessible(true);
        double[] asin = (double[]) asinF.get(null);
        Field cosF = mth.getDeclaredField("COS_TAB"); cosF.setAccessible(true);
        double[] cos = (double[]) cosF.get(null);
        System.out.println("SIN_TAB_START");
        for (float v : sin) pF(v);
        System.out.println("ASIN_TAB_START");
        for (double v : asin) pD(v);
        System.out.println("COS_TAB_START");
        for (double v : cos) pD(v);
        System.out.println("VECTORS_START");
    }
"""

FOOTER = """\
    public static void main(String[] args) throws Exception {
        tables();
        vectors();
    }
}
"""


def f32_lit(s: str) -> str:
    s = s.strip()
    if s == "f32::NAN":
        return "Float.NaN"
    if s == "f32::INFINITY":
        return "Float.POSITIVE_INFINITY"
    if s == "f32::NEG_INFINITY":
        return "Float.NEGATIVE_INFINITY"
    s = s.removesuffix("f32")
    return f"{s}F"


def f32_expr(s: str) -> str:
    """Translate a Rust f32 arg, which may be a literal or a small float
    arithmetic expression (e.g. `1.0 + 6.3`). Every numeric token gets an `F`
    suffix so Java evaluates it as float, matching the Rust f32 fn overload."""
    # Strip any trailing f32 suffix from the whole expr already present in Rust
    # (rare). Tokens: signed/unsigned floats/ints in scientific notation.
    def repl(m):
        tok = m.group(0)
        if tok.endswith("F") or tok.endswith("f"):
            return tok
        return tok + "F"

    s = s.strip()
    if s == "f32::NAN":
        return "Float.NaN"
    if s == "f32::INFINITY":
        return "Float.POSITIVE_INFINITY"
    if s == "f32::NEG_INFINITY":
        return "Float.NEGATIVE_INFINITY"
    s = s.removesuffix("f32")
    # Match numeric tokens: [+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)? but not method
    # names / identifiers. Keep the sign attached.
    return re.sub(r"[-+]?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?", repl, s)


def f64_lit(s: str) -> str:
    s = s.strip()
    if s == "f64::NAN":
        return "Double.NaN"
    if s == "f64::INFINITY":
        return "Double.POSITIVE_INFINITY"
    if s == "f64::NEG_INFINITY":
        return "Double.NEGATIVE_INFINITY"
    return s


def i32_lit(s: str) -> str:
    return s.strip().removesuffix("i32")


def i64_lit(s: str) -> str:
    s = s.strip().removesuffix("i64")
    if s == "-9223372036854775808":
        return "Long.MIN_VALUE"
    return f"{s}L"


def i8_lit(s: str) -> str:
    return f"(byte){s.strip()}"


def translate_args(args_raw: str, types: str, fn: str) -> str:
    args = split_args(args_raw)
    tlist = types.split(",")
    if len(args) != len(tlist):
        raise ValueError(f"{fn}: {len(args)} args but types {types}: {args!r}")
    out = []
    for a, t in zip(args, tlist):
        a = a.strip()
        if t == "f32":
            out.append(f32_expr(a))
        elif t == "f64":
            out.append(f64_lit(a))
        elif t == "i32":
            out.append(i32_lit(a))
        elif t == "i64":
            out.append(i64_lit(a))
        elif t == "i8":
            out.append(i8_lit(a))
        elif t == "optstr":
            if a == "None":
                out.append("null")
            elif a.startswith("Some("):
                out.append(f'"{a[5:-1].strip().strip(chr(34))}"')
            else:
                raise ValueError(f"bad optstr arg {a}")
        elif t == "rng":
            out.append(a[5:] if a.startswith("&mut ") else a)
        else:
            raise ValueError(f"unknown type {t}")
    return ", ".join(out)


def split_args(raw: str) -> list[str]:
    depth = 0
    parts = []
    cur = []
    for ch in raw:
        if ch in "({[":
            depth += 1
        elif ch in ")}]":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    if cur:
        parts.append("".join(cur))
    return parts


def print_stmt(expr: str, fmt: str) -> str:
    if fmt == "F32HEX":
        return f"pF({expr});"
    if fmt == "F64HEX":
        return f"pD({expr});"
    if fmt == "I32":
        return f"pI({expr});"
    if fmt == "I64":
        return f"pL({expr});"
    if fmt == "I8":
        return f"pB({expr});"
    if fmt == "BOOL":
        return f"pBool({expr});"
    if fmt == "VEC":
        return f"pVec({expr});"
    raise ValueError(f"bad fmt {fmt}")


def parse_sections() -> dict[int, tuple[str, str]]:
    """placeholder-index -> (open_stmt_or_None, close_stmt_or_None).

    RNG variable declarations live right after `fn golden_N() {`; emit them as a
    Java block `{ decls ... stmts ... }` so re-declared names (r1 in 17 and 18)
    stay scoped. Return per-index open/close tokens. Index is the GLOBAL
    placeholder ordinal (accumulated across all prior sections, including those
    without RNG decls, which contribute their placeholder counts).
    """
    skel = SKELETON.read_text()
    out: dict[int, tuple[str | None, str | None]] = {}
    idx = 0
    for m in re.finditer(r"fn golden_(\d+)\(\) \{", skel):
        section_start = m.start()
        section_end = skel.find("fn golden_", section_start + 1)
        if section_end < 0:
            section_end = len(skel)
        body = skel[section_start:section_end]
        n_placeholders = len(re.findall(r"@@\d+@@", body))
        # Multi-line decls (`let mut rNEG9223372036854775808 =\n    ...new(...)`) need
        # a permissive regex that spans newlines.
        decls = re.findall(
            r"let mut (\w+)\s*=\s*crate::random_source::SingleThreadedRandomSource::new\((-?\d+)i64\);",
            body,
        )
        if decls:
            open_stmt = "{"
            for var, seed in decls:
                jseed = "Long.MIN_VALUE" if seed == "-9223372036854775808" else f"{seed}L"
                open_stmt += f" RandomSource {var} = new SingleThreadedRandomSource({jseed});"
            close_stmt = "}"
            out[idx] = (open_stmt, None)
            out[idx + n_placeholders - 1] = (None, close_stmt)
        idx += n_placeholders
    return out


def emit_one(idx: int, lhs: str, rhs: str) -> str | None:
    lhs = lhs.strip()
    # The golden compares float/double bits via the NaN-canonicalizing
    # bitexact_f32/bitexact_f64 helpers (D14) instead of `.to_bits()`: unwrap
    # the helper so the inner `super::fn(...)` / rng call is translated as usual.
    m = re.match(r"bitexact_f(?:32|64)\((.*?)\)$", lhs, re.S)
    if m:
        inner = m.group(1).strip()
        # The vector lhs carries rhs as its second bitexact arg is not part of
        # lhs (it is the expected value, a placeholder in the skeleton), so
        # inner is exactly the expression whose bits we recompute.
        lhs = inner
    m = re.match(r"super::(\w+)\((.*?)\)(\.to_bits\(\))?$", lhs)
    if m:
        fn = m.group(1)
        args_raw = m.group(2)
        if fn == "clamp":
            if "i64" in args_raw:
                call = translate_args(args_raw, "i64,i64,i64", fn)
                return f"pL(Mth.clamp({call}));"
            call = translate_args(args_raw, "i32,i32,i32", fn)
            return f"pI(Mth.clamp({call}));"
        spec = SPEC.get(fn)
        if spec:
            java, types, fmt = spec
            call = translate_args(args_raw, types, fn)
            return print_stmt(f"{java}({call})", fmt)
        # else fall through to the dedicated patterns below

    m = re.match(r"super::create_insecure_uuid\(&mut crate::random_source::SingleThreadedRandomSource::new\(([^)]*)\)\)\s*\.(most|least)$", lhs, re.S)
    if m:
        seed, field = m.group(1).strip().removesuffix("i64"), m.group(2)
        jseed = "Long.MIN_VALUE" if seed == "-9223372036854775808" else f"{seed}L"
        acc = ".getMostSignificantBits()" if field == "most" else ".getLeastSignificantBits()"
        return f"pL(Mth.createInsecureUUID(new SingleThreadedRandomSource({jseed})){acc});"

    m = re.match(r"super::(out_from_origin(?:_with_step)?)\((.*)\)\.collect::<Vec<_>>\(\)$", lhs)
    if m:
        fn, args_raw = m.group(1), m.group(2)
        types = "i32,i32,i32,i32" if fn == "out_from_origin_with_step" else "i32,i32,i32"
        call = translate_args(args_raw, types, fn)
        return f"pVec(Mth.outFromOrigin({call}).toArray());"

    m = re.match(r"super::binary_search\((-?\d+), (-?\d+), (.*?)\)$", lhs)
    if m:
        frm, to, pred = m.group(1), m.group(2), m.group(3)
        jpred = pred.replace("|x|", "x ->")
        return f"pI(Mth.binarySearch({frm}, {to}, {jpred}));"

    if lhs.startswith("super::ray_intersects_aabb("):
        return RAY[idx]
    if lhs.startswith("super::rotation_around_axis("):
        return ROT[idx]

    m = re.match(r"super::mul_and_truncate\(\s*&crate::mth_stubs::Fraction\s*\{\s*numerator:\s*(-?\d+),\s*denominator:\s*(-?\d+)\s*\}\s*,\s*(\d+)\s*\)$", lhs)
    if m:
        num, den, fac = m.group(1), m.group(2), m.group(3)
        return f"pI(Mth.mulAndTruncate(Fraction.getFraction({num}, {den}), {fac}));"

    m = re.match(r"(\w+)\.(\w+)\((.*?)\)(\.to_bits\(\))?$", lhs)
    if m:
        var, meth, args_raw, to_bits = m.groups()
        spec = RNG_SPEC.get(meth)
        if not spec:
            return None
        java, fmt = spec
        if meth == "triangle_f64":
            types = "f64,f64"
        elif meth == "next_int_bound":
            types = "i32"
        elif meth == "next_int_between_inclusive":
            types = "i32,i32"
        else:
            types = ""
        if args_raw.strip():
            call = translate_args(args_raw, types, meth)
        else:
            call = ""
        return print_stmt(f"{var}.{java}({call})", fmt)

    return None


# Pre-authored Java for the complex multi-line lhs (ray/rotation).
ROT_SPECS = [
    ((0, 1, 0), (0.3, 0.4, 0.5, 0.6)),
    ((1, 0, 0), (0.1, 0.2, 0.3, 0.4)),
    ((0, 0, 1), (0.9, 0.8, 0.7, 0.6)),
    ((1, 1, 0), (0.0, 0.0, 0.0, 1.0)),
    ((0, 0, 0), (0.5, 0.5, 0.5, 0.5)),
]
ROT: dict[int, str] = {}
_i = 1124
for (ax, ay, az), (qx, qy, qz, qw) in ROT_SPECS:
    for comp in ("x", "y", "z", "w"):
        ROT[_i] = (
            f"pF(Mth.rotationAroundAxis(new Vector3f({ax}F, {ay}F, {az}F), "
            f"new Quaternionf({qx}F, {qy}F, {qz}F, {qw}F), new Quaternionf()).{comp}());"
        )
        _i += 1
assert _i == 1144, _i

RAY_SPECS = [
    ((1, 1, 1), (0, 0, 1), (0, 0, 0, 2, 2, 2)),
    ((0, 0, 0), (1, 0, 0), (0, 0, 0, 2, 2, 2)),
    ((5, 5, 5), (0, 0, 0), (0, 0, 0, 2, 2, 2)),
    ((1, 1, 1), (0, 0, -1), (0, 0, 0, 2, 2, 2)),
    ((-1, -1, -1), (1, 1, 1), (0, 0, 0, 2, 2, 2)),
    ((0, 0, 0), (-1, -1, -1), (0, 0, 0, 2, 2, 2)),
    ((2, 2, 2), (1, 0, 0), (0, 0, 0, 2, 2, 2)),
    ((0, 0, 0), (0, 0, 1), (0, 0, 0, 2, 2, 2)),
    ((1, 1, 1), (1, 1, 1), (1, 1, 1, 3, 3, 3)),
    ((1, 1, 1), (0, 0, -1), (1, 1, 1, 3, 3, 3)),
    ((3, 3, 3), (-1, -1, -1), (1, 1, 1, 3, 3, 3)),
    ((0.5, 0.5, 0.5), (1, 0, 0), (1, 1, 1, 3, 3, 3)),
]
RAY: dict[int, str] = {}
_i = 1112
for (sx, sy, sz), (dx, dy, dz), (mnx, mny, mnz, mxx, mxy, mxz) in RAY_SPECS:
    RAY[_i] = (
        f"pBool(Mth.rayIntersectsAABB(new Vec3({sx}, {sy}, {sz}), new Vec3({dx}, {dy}, {dz}), "
        f"new AABB({mnx}, {mny}, {mnz}, {mxx}, {mxy}, {mxz})));"
    )
    _i += 1
assert _i == 1124, _i


def main() -> None:
    rows = []
    for line in VECTORS.read_text().splitlines():
        idx, lhs, rhs = line.split("\t", 2)
        rows.append((int(idx), lhs, rhs))
    sections = parse_sections()

    lines = [HEADER]
    lines.append("    static void vectors() {")
    for idx, lhs, rhs in rows:
        open_stmt, close_stmt = sections.get(idx, (None, None))
        if open_stmt:
            lines.append("        " + open_stmt)
        stmt = emit_one(idx, lhs, rhs)
        if stmt is None:
            raise SystemExit(f"no translation for #{idx}: {lhs}")
        lines.append("        " + stmt)
        if close_stmt:
            lines.append("        " + close_stmt)
    lines.append("    }")
    lines.append(FOOTER)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")
    print(f"wrote {OUT} ({len(rows)} vectors)")


if __name__ == "__main__":
    main()
