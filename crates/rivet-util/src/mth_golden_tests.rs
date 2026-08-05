// GENERATED golden tests vs Java Mth oracle (working/Paper expressions). Do not hand-edit.
#![allow(clippy::all)]
#![allow(non_snake_case)]
use crate::random::RandomSource;
#[test]
fn golden_0() {
    // sin/cos
    assert_eq!(super::sin(0.0).to_bits(), 0x00000000);
    assert_eq!(super::sin(1.0).to_bits(), 0x3f57695c);
    assert_eq!(super::sin(2.0).to_bits(), 0x3f68c9b2);
    assert_eq!(super::sin(3.0).to_bits(), 0x3e108520);
    assert_eq!(super::sin(0.5).to_bits(), 0x3ef5752e);
    assert_eq!(super::sin(3.141592653589793).to_bits(), 0x250d3132);
    assert_eq!(super::sin(1.5707963267948966).to_bits(), 0x3f800000);
    assert_eq!(super::sin(6.283185307179586).to_bits(), 0x00000000);
    assert_eq!(super::sin(-3.141592653589793).to_bits(), 0x250d3132);
    assert_eq!(super::sin(-1.5707963267948966).to_bits(), 0xbf800000);
    assert_eq!(super::sin(100.0).to_bits(), 0xbf01a5b4);
    assert_eq!(super::sin(1000.0).to_bits(), 0x3f53ad24);
    assert_eq!(super::sin(10000.0).to_bits(), 0xbe9c7373);
    assert_eq!(super::sin(1000000.0).to_bits(), 0xbeb337e3);
    assert_eq!(super::sin(-1000000.0).to_bits(), 0x3eb337e3);
    assert_eq!(super::sin(12345.6789).to_bits(), 0xbf3418e1);
    assert_eq!(super::sin(1.7976931348623157E308).to_bits(), 0xb8c90fdb);
    assert_eq!(super::sin(2.2250738585072014E-308).to_bits(), 0x00000000);
    assert_eq!(super::sin(4.9E-324).to_bits(), 0x00000000);
    assert_eq!(super::sin(1.0E308).to_bits(), 0xb8c90fdb);
    assert_eq!(super::cos(0.0).to_bits(), 0x3f800000);
    assert_eq!(super::cos(1.0).to_bits(), 0x3f0a5341);
    assert_eq!(super::cos(2.0).to_bits(), 0xbed5088d);
    assert_eq!(super::cos(3.0).to_bits(), 0xbf7d7007);
    assert_eq!(super::cos(0.5).to_bits(), 0x3f60a9d2);
    assert_eq!(super::cos(3.141592653589793).to_bits(), 0xbf800000);
    assert_eq!(super::cos(1.5707963267948966).to_bits(), 0x250d3132);
    assert_eq!(super::cos(6.283185307179586).to_bits(), 0x3f800000);
    assert_eq!(super::cos(-3.141592653589793).to_bits(), 0xbf800000);
    assert_eq!(super::cos(-1.5707963267948966).to_bits(), 0x00000000);
    assert_eq!(super::cos(100.0).to_bits(), 0x3f5cbe46);
    assert_eq!(super::cos(1000.0).to_bits(), 0x3f0ff9e5);
    assert_eq!(super::cos(10000.0).to_bits(), 0xbf73c16c);
    assert_eq!(super::cos(1000000.0).to_bits(), 0x3f6fcdf4);
    assert_eq!(super::cos(-1000000.0).to_bits(), 0x3f6fcdf4);
    assert_eq!(super::cos(12345.6789).to_bits(), 0x3f35efd3);
    assert_eq!(super::cos(1.7976931348623157E308).to_bits(), 0xb8c90fdb);
    assert_eq!(super::cos(2.2250738585072014E-308).to_bits(), 0x3f800000);
    assert_eq!(super::cos(4.9E-324).to_bits(), 0x3f800000);
    assert_eq!(super::cos(1.0E308).to_bits(), 0xb8c90fdb);
}

#[test]
fn golden_1() {
    // sqrt/floor/ceil/lfloor
    assert_eq!(super::sqrt(0.0f32).to_bits(), 0x00000000);
    assert_eq!(super::sqrt(1.0f32).to_bits(), 0x3f800000);
    assert_eq!(super::sqrt(2.0f32).to_bits(), 0x3fb504f3);
    assert_eq!(super::sqrt(3.0f32).to_bits(), 0x3fddb3d7);
    assert_eq!(super::sqrt(0.25f32).to_bits(), 0x3f000000);
    assert_eq!(super::sqrt(100.0f32).to_bits(), 0x41200000);
    assert_eq!(super::sqrt(3.4028235E38f32).to_bits(), 0x5f7fffff);
    assert_eq!(super::sqrt(1.4E-45f32).to_bits(), 0x1a3504f3);
    assert_eq!(super::sqrt(f32::NAN).to_bits(), 0x7fc00000);
    assert_eq!(super::sqrt(f32::INFINITY).to_bits(), 0x7f800000);
    assert_eq!(super::sqrt(4.0f32).to_bits(), 0x40000000);
    assert_eq!(super::sqrt(1.0E30f32).to_bits(), 0x58635fa9);
    assert_eq!(super::floor_d(-1.5), -2i32);
    assert_eq!(super::floor_d(-1.0), -1i32);
    assert_eq!(super::floor_d(-0.5), -1i32);
    assert_eq!(super::floor_d(0.5), 0i32);
    assert_eq!(super::floor_d(1.5), 1i32);
    assert_eq!(super::floor_d(3.9), 3i32);
    assert_eq!(super::floor_d(-3.9), -4i32);
    assert_eq!(super::floor_d(f64::NAN), 0i32);
    assert_eq!(super::floor_d(f64::INFINITY), 2147483647i32);
    assert_eq!(super::floor_d(1.7976931348623157E308), 2147483647i32);
    assert_eq!(super::floor_d(4.9E-324), 0i32);
    assert_eq!(super::floor_d(2.147483647E9), 2147483647i32);
    assert_eq!(super::floor_d(2.147483648E9), 2147483647i32);
    assert_eq!(super::floor_d(-2.147483648E9), -2147483648i32);
    assert_eq!(super::floor_d(-2.147483649E9), -2147483648i32);
    assert_eq!(super::floor_d(1.0E30), 2147483647i32);
    assert_eq!(super::floor_d(-1.0E30), -2147483648i32);
    assert_eq!(super::floor_d(4.9E-324), 0i32);
    assert_eq!(super::floor(-1.5f32), -2i32);
    assert_eq!(super::floor(-1.0f32), -1i32);
    assert_eq!(super::floor(-0.5f32), -1i32);
    assert_eq!(super::floor(0.5f32), 0i32);
    assert_eq!(super::floor(1.5f32), 1i32);
    assert_eq!(super::floor(3.9f32), 3i32);
    assert_eq!(super::floor(-3.9f32), -4i32);
    assert_eq!(super::floor(f32::NAN), 0i32);
    assert_eq!(super::floor(3.4028235E38f32), 2147483647i32);
    assert_eq!(super::floor(1.4E-45f32), 0i32);
    assert_eq!(super::floor(2.1474836E9f32), 2147483647i32);
    assert_eq!(super::floor(2.1474836E9f32), 2147483647i32);
    assert_eq!(super::floor(-2.1474836E9f32), -2147483648i32);
    assert_eq!(super::floor(-2.1474836E9f32), -2147483648i32);
    assert_eq!(super::ceil(-1.5f32), -1i32);
    assert_eq!(super::ceil(-1.0f32), -1i32);
    assert_eq!(super::ceil(-0.5f32), 0i32);
    assert_eq!(super::ceil(0.5f32), 1i32);
    assert_eq!(super::ceil(1.5f32), 2i32);
    assert_eq!(super::ceil(3.9f32), 4i32);
    assert_eq!(super::ceil(-3.9f32), -3i32);
    assert_eq!(super::ceil(f32::NAN), 0i32);
    assert_eq!(super::ceil(3.4028235E38f32), 2147483647i32);
    assert_eq!(super::ceil(1.4E-45f32), 1i32);
    assert_eq!(super::ceil(2.1474836E9f32), 2147483647i32);
    assert_eq!(super::ceil(2.1474836E9f32), 2147483647i32);
    assert_eq!(super::ceil(-2.1474836E9f32), -2147483648i32);
    assert_eq!(super::ceil(-2.1474836E9f32), -2147483648i32);
    assert_eq!(super::ceil_d(-1.5), -1i32);
    assert_eq!(super::ceil_d(-1.0), -1i32);
    assert_eq!(super::ceil_d(-0.5), 0i32);
    assert_eq!(super::ceil_d(0.5), 1i32);
    assert_eq!(super::ceil_d(1.5), 2i32);
    assert_eq!(super::ceil_d(3.9), 4i32);
    assert_eq!(super::ceil_d(-3.9), -3i32);
    assert_eq!(super::ceil_d(f64::NAN), 0i32);
    assert_eq!(super::ceil_d(f64::INFINITY), 2147483647i32);
    assert_eq!(super::ceil_d(1.7976931348623157E308), 2147483647i32);
    assert_eq!(super::ceil_d(4.9E-324), 1i32);
    assert_eq!(super::ceil_d(2.147483647E9), 2147483647i32);
    assert_eq!(super::ceil_d(2.147483648E9), 2147483647i32);
    assert_eq!(super::ceil_d(-2.147483648E9), -2147483648i32);
    assert_eq!(super::ceil_d(-2.147483649E9), -2147483648i32);
    assert_eq!(super::ceil_d(1.0E30), 2147483647i32);
    assert_eq!(super::ceil_d(-1.0E30), -2147483648i32);
    assert_eq!(super::ceil_d(4.9E-324), 1i32);
    assert_eq!(super::lfloor(-1.5), -2i64);
    assert_eq!(super::lfloor(-1.0), -1i64);
    assert_eq!(super::lfloor(-0.5), -1i64);
    assert_eq!(super::lfloor(0.5), 0i64);
    assert_eq!(super::lfloor(1.5), 1i64);
    assert_eq!(super::lfloor(3.9), 3i64);
    assert_eq!(super::lfloor(-3.9), -4i64);
    assert_eq!(super::lfloor(f64::NAN), 0i64);
    assert_eq!(super::lfloor(f64::INFINITY), 9223372036854775807i64);
    assert_eq!(
        super::lfloor(1.7976931348623157E308),
        9223372036854775807i64
    );
    assert_eq!(super::lfloor(4.9E-324), 0i64);
    assert_eq!(super::lfloor(2.147483647E9), 2147483647i64);
    assert_eq!(super::lfloor(2.147483648E9), 2147483648i64);
    assert_eq!(super::lfloor(-2.147483648E9), -2147483648i64);
    assert_eq!(super::lfloor(-2.147483649E9), -2147483649i64);
    assert_eq!(super::lfloor(1.0E30), 9223372036854775807i64);
    assert_eq!(super::lfloor(-1.0E30), -9223372036854775808i64);
    assert_eq!(super::lfloor(4.9E-324), 0i64);
    assert_eq!(super::ceil_long(-1.5), -1i64);
    assert_eq!(super::ceil_long(-1.0), -1i64);
    assert_eq!(super::ceil_long(-0.5), 0i64);
    assert_eq!(super::ceil_long(0.5), 1i64);
    assert_eq!(super::ceil_long(1.5), 2i64);
    assert_eq!(super::ceil_long(3.9), 4i64);
    assert_eq!(super::ceil_long(-3.9), -3i64);
    assert_eq!(super::ceil_long(f64::NAN), 0i64);
    assert_eq!(super::ceil_long(f64::INFINITY), 9223372036854775807i64);
    assert_eq!(
        super::ceil_long(1.7976931348623157E308),
        9223372036854775807i64
    );
    assert_eq!(super::ceil_long(4.9E-324), 1i64);
    assert_eq!(super::ceil_long(2.147483647E9), 2147483647i64);
    assert_eq!(super::ceil_long(2.147483648E9), 2147483648i64);
    assert_eq!(super::ceil_long(-2.147483648E9), -2147483648i64);
    assert_eq!(super::ceil_long(-2.147483649E9), -2147483649i64);
    assert_eq!(super::ceil_long(1.0E30), 9223372036854775807i64);
    assert_eq!(super::ceil_long(-1.0E30), -9223372036854775808i64);
    assert_eq!(super::ceil_long(4.9E-324), 1i64);
}

#[test]
fn golden_2() {
    // abs/absMax
    assert_eq!(super::abs_i32(0i32), 0i32);
    assert_eq!(super::abs_i32(1i32), 1i32);
    assert_eq!(super::abs_i32(-1i32), 1i32);
    assert_eq!(super::abs_i32(42i32), 42i32);
    assert_eq!(super::abs_i32(-42i32), 42i32);
    assert_eq!(super::abs_i32(2147483647i32), 2147483647i32);
    assert_eq!(super::abs_i32(-2147483648i32), -2147483648i32);
    assert_eq!(super::abs_max(0i32, 1i32), 1i32);
    assert_eq!(super::abs_max(1i32, 2i32), 2i32);
    assert_eq!(super::abs_max(-1i32, 0i32), 1i32);
    assert_eq!(super::abs_max(42i32, 43i32), 43i32);
    assert_eq!(super::abs_max(-42i32, -41i32), 42i32);
    assert_eq!(super::abs_max(2147483647i32, -2147483648i32), 2147483647i32);
    assert_eq!(
        super::abs_max(-2147483648i32, -2147483647i32),
        2147483647i32
    );
    assert_eq!(super::abs_max(0i32, -1i32), 1i32);
    assert_eq!(super::abs_max(1i32, 0i32), 1i32);
    assert_eq!(super::abs_max(-1i32, -2i32), 2i32);
    assert_eq!(super::abs_max(42i32, 41i32), 42i32);
    assert_eq!(super::abs_max(-42i32, -43i32), 43i32);
    assert_eq!(super::abs_max(2147483647i32, 2147483646i32), 2147483647i32);
    assert_eq!(super::abs_max(-2147483648i32, 2147483647i32), 2147483647i32);
    assert_eq!(super::chessboard_distance(3, 7, -2, 10), 5i32);
    assert_eq!(super::chessboard_distance(-5, 0, 5, 0), 10i32);
    assert_eq!(super::abs(-1.5f32).to_bits(), 0x3fc00000);
    assert_eq!(super::abs(1.5f32).to_bits(), 0x3fc00000);
    assert_eq!(super::abs(f32::NAN).to_bits(), 0x7fc00000);
    assert_eq!(super::abs(1.4E-45f32).to_bits(), 0x00000001);
}

#[test]
fn golden_3() {
    // clamp
    assert_eq!(super::clamp(5, 0, 10), 5i32);
    assert_eq!(super::clamp(-5, 0, 10), 0i32);
    assert_eq!(super::clamp(15, 0, 10), 10i32);
    assert_eq!(super::clamp(-2147483648, -1, 1), -1i32);
    assert_eq!(super::clamp(2147483647, -1, 1), 1i32);
    assert_eq!(super::clamp(-9223372036854775808i64, -1i64, 1i64), -1i64);
    assert_eq!(super::clamp_f32(5.0f32, 0.0, 10.0).to_bits(), 0x40a00000);
    assert_eq!(super::clamp_f32(-5.0f32, 0.0, 10.0).to_bits(), 0x00000000);
    assert_eq!(super::clamp_f32(15.0f32, 0.0, 10.0).to_bits(), 0x41200000);
    assert_eq!(super::clamp_f32(f32::NAN, 0.0, 10.0).to_bits(), 0x7fc00000);
    assert_eq!(
        super::clamp_f32(5.0f32, 0.0, f32::NAN).to_bits(),
        0x7fc00000
    );
    assert_eq!(super::clamp_f32(0.5f32, 0.0, 10.0).to_bits(), 0x3f000000);
    assert_eq!(
        super::clamp_f64(5.0, 0.0, 10.0).to_bits(),
        0x4014000000000000
    );
    assert_eq!(
        super::clamp_f64(-5.0, 0.0, 10.0).to_bits(),
        0x0000000000000000
    );
    assert_eq!(
        super::clamp_f64(15.0, 0.0, 10.0).to_bits(),
        0x4024000000000000
    );
    assert_eq!(
        super::clamp_f64(f64::NAN, 0.0, 10.0).to_bits(),
        0x7ff8000000000000
    );
    assert_eq!(
        super::clamp_f64(5.0, 0.0, f64::NAN).to_bits(),
        0x7ff8000000000000
    );
    assert_eq!(
        super::clamp_f64(0.5, 0.0, 10.0).to_bits(),
        0x3fe0000000000000
    );
}

#[test]
fn golden_4() {
    // clampedLerp/lerp/lerpInt/lerpDiscrete
    assert_eq!(
        super::clamped_lerp(-1.0, 1.0, 5.0).to_bits(),
        0x3ff0000000000000
    );
    assert_eq!(
        super::clamped_lerp(0.0, 1.0, 5.0).to_bits(),
        0x3ff0000000000000
    );
    assert_eq!(
        super::clamped_lerp(0.25, 1.0, 5.0).to_bits(),
        0x4000000000000000
    );
    assert_eq!(
        super::clamped_lerp(0.5, 1.0, 5.0).to_bits(),
        0x4008000000000000
    );
    assert_eq!(
        super::clamped_lerp(0.75, 1.0, 5.0).to_bits(),
        0x4010000000000000
    );
    assert_eq!(
        super::clamped_lerp(1.0, 1.0, 5.0).to_bits(),
        0x4014000000000000
    );
    assert_eq!(
        super::clamped_lerp(2.0, 1.0, 5.0).to_bits(),
        0x4014000000000000
    );
    assert_eq!(
        super::clamped_lerp(f64::NAN, 1.0, 5.0).to_bits(),
        0x7ff8000000000000
    );
    assert_eq!(
        super::clamped_lerp_f32(-1.0f32, 1.0, 5.0).to_bits(),
        0x3f800000
    );
    assert_eq!(
        super::clamped_lerp_f32(0.0f32, 1.0, 5.0).to_bits(),
        0x3f800000
    );
    assert_eq!(
        super::clamped_lerp_f32(0.25f32, 1.0, 5.0).to_bits(),
        0x40000000
    );
    assert_eq!(
        super::clamped_lerp_f32(0.5f32, 1.0, 5.0).to_bits(),
        0x40400000
    );
    assert_eq!(
        super::clamped_lerp_f32(1.0f32, 1.0, 5.0).to_bits(),
        0x40a00000
    );
    assert_eq!(
        super::clamped_lerp_f32(2.0f32, 1.0, 5.0).to_bits(),
        0x40a00000
    );
    assert_eq!(
        super::clamped_lerp_f32(f32::NAN, 1.0, 5.0).to_bits(),
        0x7fc00000
    );
    assert_eq!(super::lerp(-1.0, 1.0, 5.0).to_bits(), 0xc008000000000000);
    assert_eq!(super::lerp(0.0, 1.0, 5.0).to_bits(), 0x3ff0000000000000);
    assert_eq!(super::lerp(0.5, 1.0, 5.0).to_bits(), 0x4008000000000000);
    assert_eq!(super::lerp(1.0, 1.0, 5.0).to_bits(), 0x4014000000000000);
    assert_eq!(super::lerp(2.0, 1.0, 5.0).to_bits(), 0x4022000000000000);
    assert_eq!(
        super::lerp(f64::NAN, 1.0, 5.0).to_bits(),
        0x7ff8000000000000
    );
    assert_eq!(super::lerp_f32(-1.0f32, 1.0, 5.0).to_bits(), 0xc0400000);
    assert_eq!(super::lerp_f32(0.0f32, 1.0, 5.0).to_bits(), 0x3f800000);
    assert_eq!(super::lerp_f32(0.5f32, 1.0, 5.0).to_bits(), 0x40400000);
    assert_eq!(super::lerp_f32(1.0f32, 1.0, 5.0).to_bits(), 0x40a00000);
    assert_eq!(super::lerp_f32(f32::NAN, 1.0, 5.0).to_bits(), 0x7fc00000);
    assert_eq!(super::lerp_int(0.5, 10, 20), 15i32);
    assert_eq!(super::lerp_int(-0.5, 10, 20), 5i32);
    assert_eq!(super::lerp_int(0.0, 10, 20), 10i32);
    assert_eq!(super::lerp_int(1.0, 10, 20), 20i32);
    assert_eq!(super::lerp_discrete(0.5, 10, 20), 15i32);
    assert_eq!(super::lerp_discrete(0.0, 10, 20), 10i32);
    assert_eq!(super::lerp_discrete(0.5, 10, 11), 11i32);
    assert_eq!(
        super::lerp2(0.5, 0.5, 0.0, 10.0, 20.0, 30.0).to_bits(),
        0x402e000000000000
    );
    assert_eq!(
        super::lerp3(0.5, 0.5, 0.5, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0).to_bits(),
        0x400c000000000000
    );
}

#[test]
fn golden_5() {
    // wrapDegrees(int)
    assert_eq!(super::wrap_degrees(180i32), -180i32);
    assert_eq!(super::wrap_degrees(-180i32), -180i32);
    assert_eq!(super::wrap_degrees(181i32), -179i32);
    assert_eq!(super::wrap_degrees(-181i32), 179i32);
    assert_eq!(super::wrap_degrees(360i32), 0i32);
    assert_eq!(super::wrap_degrees(-360i32), 0i32);
    assert_eq!(super::wrap_degrees(540i32), -180i32);
    assert_eq!(super::wrap_degrees(-540i32), -180i32);
    assert_eq!(super::wrap_degrees(720i32), 0i32);
    assert_eq!(super::wrap_degrees(0i32), 0i32);
    assert_eq!(super::wrap_degrees(359i32), -1i32);
    assert_eq!(super::wrap_degrees(-359i32), 1i32);
    assert_eq!(super::wrap_degrees(179i32), 179i32);
    assert_eq!(super::wrap_degrees(-179i32), -179i32);
    assert_eq!(super::wrap_degrees(2147483647i32), 127i32);
    assert_eq!(super::wrap_degrees(-2147483648i32), -128i32);
    assert_eq!(super::wrap_degrees(1000000i32), -80i32);
    assert_eq!(super::wrap_degrees(-1000000i32), 80i32);
}

#[test]
fn golden_6() {
    // wrapDegrees(float)
    assert_eq!(super::wrap_degrees_f32(180.0f32).to_bits(), 0xc3340000);
    assert_eq!(super::wrap_degrees_f32(-180.0f32).to_bits(), 0xc3340000);
    assert_eq!(super::wrap_degrees_f32(181.0f32).to_bits(), 0xc3330000);
    assert_eq!(super::wrap_degrees_f32(-181.0f32).to_bits(), 0x43330000);
    assert_eq!(super::wrap_degrees_f32(360.0f32).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees_f32(-360.0f32).to_bits(), 0x80000000);
    assert_eq!(super::wrap_degrees_f32(540.0f32).to_bits(), 0xc3340000);
    assert_eq!(super::wrap_degrees_f32(-540.0f32).to_bits(), 0xc3340000);
    assert_eq!(super::wrap_degrees_f32(720.0f32).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees_f32(0.0f32).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees_f32(359.9f32).to_bits(), 0xbdccd000);
    assert_eq!(super::wrap_degrees_f32(-359.9f32).to_bits(), 0x3dccd000);
    assert_eq!(super::wrap_degrees_f32(179.999f32).to_bits(), 0x4333ffbe);
    assert_eq!(super::wrap_degrees_f32(f32::NAN).to_bits(), 0x7fc00000);
    assert_eq!(
        super::wrap_degrees_f32(3.4028235E38f32).to_bits(),
        0x00000000
    );
    assert_eq!(
        super::wrap_degrees_f32(-3.4028235E38f32).to_bits(),
        0x80000000
    );
    assert_eq!(super::wrap_degrees_f32(1.4E-45f32).to_bits(), 0x00000001);
    assert_eq!(super::wrap_degrees_f32(-1.4E-45f32).to_bits(), 0x80000001);
    assert_eq!(super::wrap_degrees_f32(-0.0f32).to_bits(), 0x80000000);
    assert_eq!(super::wrap_degrees_f32(1.0E-4f32).to_bits(), 0x38d1b717);
    assert_eq!(super::wrap_degrees_f32(270.0f32).to_bits(), 0xc2b40000);
    assert_eq!(super::wrap_degrees_f32(-270.0f32).to_bits(), 0x42b40000);
    assert_eq!(super::wrap_degrees_f32(90.0f32).to_bits(), 0x42b40000);
    assert_eq!(super::wrap_degrees_f32(-90.0f32).to_bits(), 0xc2b40000);
    assert_eq!(super::wrap_degrees_i64(180i64).to_bits(), 0xc3340000);
    assert_eq!(super::wrap_degrees_i64(-180i64).to_bits(), 0xc3340000);
    assert_eq!(super::wrap_degrees_i64(181i64).to_bits(), 0xc3330000);
    assert_eq!(super::wrap_degrees_i64(-181i64).to_bits(), 0x43330000);
    assert_eq!(super::wrap_degrees_i64(360i64).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees_i64(-360i64).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees_i64(540i64).to_bits(), 0xc3340000);
    assert_eq!(super::wrap_degrees_i64(-540i64).to_bits(), 0xc3340000);
    assert_eq!(
        super::wrap_degrees_i64(9223372036854775807i64).to_bits(),
        0x40e00000
    );
    assert_eq!(
        super::wrap_degrees_i64(-9223372036854775808i64).to_bits(),
        0xc1000000
    );
    assert_eq!(super::wrap_degrees_i64(0i64).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees_i64(359i64).to_bits(), 0xbf800000);
    assert_eq!(super::wrap_degrees_i64(1i64).to_bits(), 0x3f800000);
    assert_eq!(super::wrap_degrees_f64(180.0).to_bits(), 0xc066800000000000);
    assert_eq!(
        super::wrap_degrees_f64(-180.0).to_bits(),
        0xc066800000000000
    );
    assert_eq!(super::wrap_degrees_f64(181.0).to_bits(), 0xc066600000000000);
    assert_eq!(
        super::wrap_degrees_f64(-181.0).to_bits(),
        0x4066600000000000
    );
    assert_eq!(super::wrap_degrees_f64(360.0).to_bits(), 0x0000000000000000);
    assert_eq!(
        super::wrap_degrees_f64(-360.0).to_bits(),
        0x8000000000000000
    );
    assert_eq!(super::wrap_degrees_f64(540.0).to_bits(), 0xc066800000000000);
    assert_eq!(
        super::wrap_degrees_f64(-540.0).to_bits(),
        0xc066800000000000
    );
    assert_eq!(
        super::wrap_degrees_f64(179.999).to_bits(),
        0x40667ff7ced91687
    );
    assert_eq!(
        super::wrap_degrees_f64(1.0E18).to_bits(),
        0xc054000000000000
    );
    assert_eq!(
        super::wrap_degrees_f64(-1.0E18).to_bits(),
        0x4054000000000000
    );
    assert_eq!(
        super::wrap_degrees_f64(f64::NAN).to_bits(),
        0x7ff8000000000000
    );
    assert_eq!(
        super::wrap_degrees_f64(f64::INFINITY).to_bits(),
        0x7ff8000000000000
    );
    assert_eq!(super::wrap_degrees_f64(0.0).to_bits(), 0x0000000000000000);
    assert_eq!(super::wrap_degrees_f64(-0.0).to_bits(), 0x8000000000000000);
    assert_eq!(super::wrap_degrees_f64(90.0).to_bits(), 0x4056800000000000);
    assert_eq!(super::wrap_degrees_f64(270.0).to_bits(), 0xc056800000000000);
    assert_eq!(super::wrap_degrees90(45.0f32).to_bits(), 0xc2340000);
    assert_eq!(super::wrap_degrees90(-45.0f32).to_bits(), 0xc2340000);
    assert_eq!(super::wrap_degrees90(90.0f32).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees90(-90.0f32).to_bits(), 0x80000000);
    assert_eq!(super::wrap_degrees90(44.0f32).to_bits(), 0x42300000);
    assert_eq!(super::wrap_degrees90(-44.0f32).to_bits(), 0xc2300000);
    assert_eq!(super::wrap_degrees90(46.0f32).to_bits(), 0xc2300000);
    assert_eq!(super::wrap_degrees90(-46.0f32).to_bits(), 0x42300000);
    assert_eq!(super::wrap_degrees90(135.0f32).to_bits(), 0xc2340000);
    assert_eq!(super::wrap_degrees90(-135.0f32).to_bits(), 0xc2340000);
    assert_eq!(super::wrap_degrees90(180.0f32).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees90(-180.0f32).to_bits(), 0x80000000);
    assert_eq!(super::wrap_degrees90(0.0f32).to_bits(), 0x00000000);
    assert_eq!(super::wrap_degrees90(f32::NAN).to_bits(), 0x7fc00000);
    assert_eq!(super::wrap_degrees90(89.0f32).to_bits(), 0xbf800000);
    assert_eq!(super::wrap_degrees90(179.0f32).to_bits(), 0xbf800000);
}

#[test]
fn golden_7() {
    // degreesDifference/rotateIfNecessary/approach
    assert_eq!(super::degrees_difference(10.0, 350.0).to_bits(), 0xc1a00000);
    assert_eq!(super::degrees_difference(350.0, 10.0).to_bits(), 0x41a00000);
    assert_eq!(super::degrees_difference(0.0, 180.0).to_bits(), 0xc3340000);
    assert_eq!(
        super::degrees_difference_abs(10.0, 350.0).to_bits(),
        0x41a00000
    );
    assert_eq!(
        super::rotate_if_necessary(10.0, 350.0, 30.0).to_bits(),
        0x43b90000
    );
    assert_eq!(
        super::rotate_if_necessary(0.0, 20.0, 90.0).to_bits(),
        0x00000000
    );
    assert_eq!(super::approach(0.0, 10.0, 3.0).to_bits(), 0x40400000);
    assert_eq!(super::approach(10.0, 0.0, 3.0).to_bits(), 0x40e00000);
    assert_eq!(super::approach(5.0, 5.0, 3.0).to_bits(), 0x40a00000);
    assert_eq!(super::approach(0.0, 10.0, -3.0).to_bits(), 0x40400000);
    assert_eq!(
        super::approach_degrees(10.0, 350.0, 45.0).to_bits(),
        0xc1200000
    );
}

#[test]
fn golden_8() {
    // getInt / smallestSquareSide / powerOfTwo
    assert_eq!(super::get_int(Some("42"), -1), 42i32);
    assert_eq!(super::get_int(Some("-42"), -1), -42i32);
    assert_eq!(super::get_int(Some("+42"), -1), 42i32);
    assert_eq!(super::get_int(Some(""), -1), -1i32);
    assert_eq!(super::get_int(Some(" "), -1), -1i32);
    assert_eq!(super::get_int(Some(" 42"), -1), -1i32);
    assert_eq!(super::get_int(Some("42 "), -1), -1i32);
    assert_eq!(super::get_int(Some("42abc"), -1), -1i32);
    assert_eq!(super::get_int(Some("abc"), -1), -1i32);
    assert_eq!(super::get_int(Some("2147483647"), -1), 2147483647i32);
    assert_eq!(super::get_int(Some("2147483648"), -1), -1i32);
    assert_eq!(super::get_int(Some("-2147483648"), -1), -2147483648i32);
    assert_eq!(super::get_int(Some("-2147483649"), -1), -1i32);
    assert_eq!(super::get_int(Some("9223372036854775808"), -1), -1i32);
    assert_eq!(super::get_int(Some("9223372036854775807"), -1), -1i32);
    assert_eq!(super::get_int(Some("18446744073709551616"), -1), -1i32);
    assert_eq!(super::get_int(Some("0x10"), -1), -1i32);
    assert_eq!(super::get_int(Some("1e3"), -1), -1i32);
    assert_eq!(super::get_int(None, -1), -1i32);
    assert_eq!(super::smallest_square_side(0), 0i32);
    assert_eq!(super::smallest_square_side(1), 1i32);
    assert_eq!(super::smallest_square_side(2), 2i32);
    assert_eq!(super::smallest_square_side(3), 2i32);
    assert_eq!(super::smallest_square_side(4), 2i32);
    assert_eq!(super::smallest_square_side(5), 3i32);
    assert_eq!(super::smallest_square_side(8), 3i32);
    assert_eq!(super::smallest_square_side(9), 3i32);
    assert_eq!(super::smallest_square_side(10), 4i32);
    assert_eq!(super::smallest_square_side(15), 4i32);
    assert_eq!(super::smallest_square_side(16), 4i32);
    assert_eq!(super::smallest_square_side(17), 5i32);
    assert_eq!(super::smallest_square_side(1023), 32i32);
    assert_eq!(super::smallest_square_side(1024), 32i32);
    assert_eq!(super::smallest_square_side(1025), 33i32);
    assert_eq!(super::smallest_square_side(2147483647), 46341i32);
    assert_eq!(super::smallest_encompassing_power_of_two(0), 0i32);
    assert_eq!(super::smallest_encompassing_power_of_two(1), 1i32);
    assert_eq!(super::smallest_encompassing_power_of_two(2), 2i32);
    assert_eq!(super::smallest_encompassing_power_of_two(3), 4i32);
    assert_eq!(super::smallest_encompassing_power_of_two(4), 4i32);
    assert_eq!(super::smallest_encompassing_power_of_two(5), 8i32);
    assert_eq!(super::smallest_encompassing_power_of_two(8), 8i32);
    assert_eq!(super::smallest_encompassing_power_of_two(9), 16i32);
    assert_eq!(super::smallest_encompassing_power_of_two(16), 16i32);
    assert_eq!(super::smallest_encompassing_power_of_two(31), 32i32);
    assert_eq!(super::smallest_encompassing_power_of_two(32), 32i32);
    assert_eq!(super::smallest_encompassing_power_of_two(1023), 1024i32);
    assert_eq!(super::smallest_encompassing_power_of_two(1024), 1024i32);
    assert_eq!(
        super::smallest_encompassing_power_of_two(2147483647),
        -2147483648i32
    );
    assert_eq!(
        super::smallest_encompassing_power_of_two(-2147483648),
        -2147483648i32
    );
    assert_eq!(super::smallest_encompassing_power_of_two(-1), 0i32);
    assert_eq!(super::smallest_encompassing_power_of_two(-2), 0i32);
    assert_eq!(super::is_power_of_two(0i32), false);
    assert_eq!(super::is_power_of_two(1i32), true);
    assert_eq!(super::is_power_of_two(2i32), true);
    assert_eq!(super::is_power_of_two(3i32), false);
    assert_eq!(super::is_power_of_two(4i32), true);
    assert_eq!(super::is_power_of_two(7i32), false);
    assert_eq!(super::is_power_of_two(8i32), true);
    assert_eq!(super::is_power_of_two(9i32), false);
    assert_eq!(super::is_power_of_two(1024i32), true);
    assert_eq!(super::is_power_of_two(-2147483648i32), true);
    assert_eq!(super::is_power_of_two(-1i32), false);
    assert_eq!(super::is_power_of_two(-2i32), false);
    assert_eq!(super::is_power_of_two_i64(0i64), false);
    assert_eq!(super::is_power_of_two_i64(1i64), true);
    assert_eq!(super::is_power_of_two_i64(2i64), true);
    assert_eq!(super::is_power_of_two_i64(3i64), false);
    assert_eq!(super::is_power_of_two_i64(4i64), true);
    assert_eq!(super::is_power_of_two_i64(7i64), false);
    assert_eq!(super::is_power_of_two_i64(8i64), true);
    assert_eq!(super::is_power_of_two_i64(9i64), false);
    assert_eq!(super::is_power_of_two_i64(1024i64), true);
    assert_eq!(super::is_power_of_two_i64(-9223372036854775808i64), true);
    assert_eq!(super::is_power_of_two_i64(-1i64), false);
    assert_eq!(super::is_power_of_two_i64(4611686018427387904i64), true);
    assert_eq!(super::is_power_of_two_i64(-9223372036854775808i64), true);
    assert_eq!(super::ceillog2(0), 0i32);
    assert_eq!(super::ceillog2(1), 0i32);
    assert_eq!(super::ceillog2(2), 1i32);
    assert_eq!(super::ceillog2(3), 2i32);
    assert_eq!(super::ceillog2(4), 2i32);
    assert_eq!(super::ceillog2(5), 3i32);
    assert_eq!(super::ceillog2(8), 3i32);
    assert_eq!(super::ceillog2(9), 4i32);
    assert_eq!(super::ceillog2(16), 4i32);
    assert_eq!(super::ceillog2(17), 5i32);
    assert_eq!(super::ceillog2(1023), 10i32);
    assert_eq!(super::ceillog2(1024), 10i32);
    assert_eq!(super::ceillog2(1025), 11i32);
    assert_eq!(super::ceillog2(2147483647), 31i32);
    assert_eq!(super::ceillog2(-2147483648), 31i32);
    assert_eq!(super::ceillog2(7), 3i32);
    assert_eq!(super::ceillog2(6), 3i32);
    assert_eq!(super::log2(0), -1i32);
    assert_eq!(super::log2(1), 0i32);
    assert_eq!(super::log2(2), 1i32);
    assert_eq!(super::log2(3), 1i32);
    assert_eq!(super::log2(4), 2i32);
    assert_eq!(super::log2(5), 2i32);
    assert_eq!(super::log2(8), 3i32);
    assert_eq!(super::log2(9), 3i32);
    assert_eq!(super::log2(16), 4i32);
    assert_eq!(super::log2(17), 4i32);
    assert_eq!(super::log2(1023), 9i32);
    assert_eq!(super::log2(1024), 10i32);
    assert_eq!(super::log2(1025), 10i32);
    assert_eq!(super::log2(2147483647), 30i32);
    assert_eq!(super::log2(-2147483648), 31i32);
    assert_eq!(super::log2(7), 2i32);
    assert_eq!(super::log2(6), 2i32);
}

#[test]
fn golden_9() {
    // frac/getSeed/murmur
    assert_eq!(super::frac(1.5f32).to_bits(), 0x3f000000);
    assert_eq!(super::frac(-1.5f32).to_bits(), 0x3f000000);
    assert_eq!(super::frac(0.5f32).to_bits(), 0x3f000000);
    assert_eq!(super::frac(-0.5f32).to_bits(), 0x3f000000);
    assert_eq!(super::frac(3.9f32).to_bits(), 0x3f666668);
    assert_eq!(super::frac(-3.9f32).to_bits(), 0x3dccccc0);
    assert_eq!(super::frac(0.0f32).to_bits(), 0x00000000);
    assert_eq!(super::frac(-0.0f32).to_bits(), 0x80000000);
    assert_eq!(super::frac(f32::NAN).to_bits(), 0x7fc00000);
    assert_eq!(super::frac_f64(1.5).to_bits(), 0x3fe0000000000000);
    assert_eq!(super::frac_f64(-1.5).to_bits(), 0x3fe0000000000000);
    assert_eq!(super::frac_f64(0.5).to_bits(), 0x3fe0000000000000);
    assert_eq!(super::frac_f64(-0.5).to_bits(), 0x3fe0000000000000);
    assert_eq!(super::frac_f64(3.9).to_bits(), 0x3feccccccccccccc);
    assert_eq!(super::frac_f64(-3.9).to_bits(), 0x3fb99999999999a0);
    assert_eq!(super::frac_f64(0.0).to_bits(), 0x0000000000000000);
    assert_eq!(super::frac_f64(-0.0).to_bits(), 0x8000000000000000);
    assert_eq!(super::frac_f64(f64::NAN).to_bits(), 0x7ff8000000000000);
    assert_eq!(super::frac_f64(1.0E20).to_bits(), 0x4413af1d78b58c40);
    assert_eq!(super::frac_f64(-1.0E20).to_bits(), 0xc413af1d78b58c40);
    assert_eq!(super::get_seed(1, 2, 3), -33674130277896i64);
    assert_eq!(super::get_seed(0, 0, 0), 0i64);
    assert_eq!(super::get_seed(-1, 1, -1), 60311958971344i64);
    assert_eq!(
        super::get_seed(2147483647, -2147483648, 0),
        133076631896896i64
    );
    assert_eq!(super::get_seed(7, 7, 7), -35564658949879i64);
    assert_eq!(super::get_seed(12345, -9876, 55555), 58516538991611i64);
    assert_eq!(super::murmur_hash3_mixer(0), 0i32);
    assert_eq!(super::murmur_hash3_mixer(1), 1364076727i32);
    assert_eq!(super::murmur_hash3_mixer(-1), -2114883783i32);
    assert_eq!(super::murmur_hash3_mixer(2147483647), -104067416i32);
    assert_eq!(super::murmur_hash3_mixer(-2147483648), 1832674720i32);
    assert_eq!(super::murmur_hash3_mixer(12345), 1011272156i32);
    assert_eq!(super::murmur_hash3_mixer(-12345), -413722947i32);
    assert_eq!(super::murmur_hash3_mixer(987654321), 1443973408i32);
}

#[test]
fn golden_10() {
    // positiveModulo/isMultipleOf/floorDiv
    assert_eq!(super::positive_modulo(7, 3), 1i32);
    assert_eq!(super::positive_modulo(-7, 3), 2i32);
    assert_eq!(super::positive_modulo(7, -3), -2i32);
    assert_eq!(super::positive_modulo(-7, -3), -1i32);
    assert_eq!(super::positive_modulo(-2147483648, 3), 1i32);
    assert_eq!(super::positive_modulo(2147483647, 2), 1i32);
    assert_eq!(super::positive_modulo(0, 5), 0i32);
    assert_eq!(super::positive_modulo(-1, 5), 4i32);
    assert_eq!(super::positive_modulo(-6, 5), 4i32);
    assert_eq!(super::positive_modulo(6, 5), 1i32);
    assert_eq!(super::floor_div(7, 3), 2i32);
    assert_eq!(super::floor_div(-7, 3), -3i32);
    assert_eq!(super::floor_div(7, -3), -3i32);
    assert_eq!(super::floor_div(-7, -3), 2i32);
    assert_eq!(super::floor_div(-2147483648, 3), -715827883i32);
    assert_eq!(super::floor_div(2147483647, 2), 1073741823i32);
    assert_eq!(super::floor_div(0, 5), 0i32);
    assert_eq!(super::floor_div(-1, 5), -1i32);
    assert_eq!(super::floor_div(-6, 5), -2i32);
    assert_eq!(super::floor_div(6, 5), 1i32);
    assert_eq!(super::is_multiple_of(7, 3), false);
    assert_eq!(super::is_multiple_of(-7, 3), false);
    assert_eq!(super::is_multiple_of(7, -3), false);
    assert_eq!(super::is_multiple_of(-7, -3), false);
    assert_eq!(super::is_multiple_of(-2147483648, 3), false);
    assert_eq!(super::is_multiple_of(2147483647, 2), false);
    assert_eq!(super::is_multiple_of(0, 5), true);
    assert_eq!(super::is_multiple_of(-1, 5), false);
    assert_eq!(super::is_multiple_of(-6, 5), false);
    assert_eq!(super::is_multiple_of(6, 5), false);
    assert_eq!(
        super::positive_modulo_f32(7.5f32, 3.0f32).to_bits(),
        0x3fc00000
    );
    assert_eq!(
        super::positive_modulo_f32(-7.5f32, 3.0f32).to_bits(),
        0x3fc00000
    );
    assert_eq!(
        super::positive_modulo_f32(7.5f32, -3.0f32).to_bits(),
        0xbfc00000
    );
    assert_eq!(
        super::positive_modulo_f32(-7.5f32, -3.0f32).to_bits(),
        0xbfc00000
    );
    assert_eq!(
        super::positive_modulo_f32(0.0f32, 3.0f32).to_bits(),
        0x00000000
    );
    assert_eq!(
        super::positive_modulo_f32(-0.0f32, 3.0f32).to_bits(),
        0x00000000
    );
    assert_eq!(
        super::positive_modulo_f64(7.5, 3.0).to_bits(),
        0x3ff8000000000000
    );
    assert_eq!(
        super::positive_modulo_f64(-7.5, 3.0).to_bits(),
        0x3ff8000000000000
    );
    assert_eq!(
        super::positive_modulo_f64(7.5, -3.0).to_bits(),
        0xbff8000000000000
    );
    assert_eq!(
        super::positive_modulo_f64(-7.5, -3.0).to_bits(),
        0xbff8000000000000
    );
    assert_eq!(
        super::positive_modulo_f64(0.0, 3.0).to_bits(),
        0x0000000000000000
    );
    assert_eq!(
        super::positive_modulo_f64(-0.0, 3.0).to_bits(),
        0x0000000000000000
    );
}

#[test]
fn golden_11() {
    // packDegrees/unpackDegrees
    assert_eq!(super::pack_degrees(0.0f32), 0);
    assert_eq!(super::pack_degrees(45.0f32), 32);
    assert_eq!(super::pack_degrees(90.0f32), 64);
    assert_eq!(super::pack_degrees(180.0f32), -128);
    assert_eq!(super::pack_degrees(270.0f32), -64);
    assert_eq!(super::pack_degrees(359.0f32), -1);
    assert_eq!(super::pack_degrees(-45.0f32), -32);
    assert_eq!(super::pack_degrees(-90.0f32), -64);
    assert_eq!(super::pack_degrees(-180.0f32), -128);
    assert_eq!(super::pack_degrees(-270.0f32), 64);
    assert_eq!(super::pack_degrees(360.0f32), 0);
    assert_eq!(super::pack_degrees(720.0f32), 0);
    assert_eq!(super::pack_degrees(f32::NAN), 0);
    assert_eq!(super::pack_degrees(1.0f32), 0);
    assert_eq!(super::pack_degrees(-1.0f32), -1);
    assert_eq!(super::unpack_degrees(0).to_bits(), 0x00000000);
    assert_eq!(super::unpack_degrees(45).to_bits(), 0x427d2000);
    assert_eq!(super::unpack_degrees(90).to_bits(), 0x42fd2000);
    assert_eq!(super::unpack_degrees(-1).to_bits(), 0xbfb40000);
    assert_eq!(super::unpack_degrees(-90).to_bits(), 0xc2fd2000);
    assert_eq!(super::unpack_degrees(-128).to_bits(), 0xc3340000);
    assert_eq!(super::unpack_degrees(127).to_bits(), 0x43329800);
    assert_eq!(super::unpack_degrees(1).to_bits(), 0x3fb40000);
    assert_eq!(super::unpack_degrees(-2).to_bits(), 0xc0340000);
}

#[test]
fn golden_12() {
    // atan2
    assert_eq!(super::atan2(0.0, 0.0).to_bits(), 0x0000000000000000);
    assert_eq!(super::atan2(0.0, 1.0).to_bits(), 0x0000000000000000);
    assert_eq!(super::atan2(1.0, 0.0).to_bits(), 0x3ff921fb54442d18);
    assert_eq!(super::atan2(-1.0, 0.0).to_bits(), 0xbff921fb54442d18);
    assert_eq!(super::atan2(0.0, -1.0).to_bits(), 0x400921fb54442d18);
    assert_eq!(super::atan2(1.0, 1.0).to_bits(), 0x3fe921fb45e6d12f);
    assert_eq!(super::atan2(-1.0, -1.0).to_bits(), 0xc002d97c82ca78cc);
    assert_eq!(super::atan2(1.0, -1.0).to_bits(), 0x4002d97c82ca78cc);
    assert_eq!(super::atan2(-1.0, 1.0).to_bits(), 0xbfe921fb45e6d12f);
    assert_eq!(super::atan2(3.0, 4.0).to_bits(), 0x3fe497861c1ebb2e);
    assert_eq!(super::atan2(4.0, 3.0).to_bits(), 0x3fedac708c699f02);
    assert_eq!(super::atan2(0.1, 0.2).to_bits(), 0x3fddac64ca13863b);
    assert_eq!(super::atan2(-0.1, 0.2).to_bits(), 0xbfddac64ca13863b);
    assert_eq!(super::atan2(0.1, -0.2).to_bits(), 0x40056c6ebb01bc51);
    assert_eq!(super::atan2(-0.1, -0.2).to_bits(), 0xc0056c6ebb01bc51);
    assert_eq!(super::atan2(f64::NAN, 1.0).to_bits(), 0x7ff8000000000000);
    assert_eq!(super::atan2(1.0, f64::NAN).to_bits(), 0x7ff8000000000000);
    assert_eq!(
        super::atan2(f64::INFINITY, 1.0).to_bits(),
        0x7ff0000000000000
    );
    assert_eq!(
        super::atan2(1.0, f64::INFINITY).to_bits(),
        0xfff0000000000000
    );
    assert_eq!(
        super::atan2(1.7976931348623157E308, 4.9E-324).to_bits(),
        0x7ff0000000000000
    );
    assert_eq!(super::atan2(1.0E300, 1.0E300).to_bits(), 0xfff0000000000000);
    assert_eq!(
        super::atan2(1.0E-300, 1.0E-300).to_bits(),
        0x21a705f2fc84defa
    );
    assert_eq!(
        super::atan2(12345.678, 98765.432).to_bits(),
        0x3fbfd5d00b34bc34
    );
    assert_eq!(super::atan2(0.5, 0.5).to_bits(), 0x3fe921fb45e6d12f);
    assert_eq!(super::atan2(-0.5, 0.5).to_bits(), 0xbfe921fb45e6d12f);
    assert_eq!(super::atan2(0.5, -0.5).to_bits(), 0x4002d97c82ca78cc);
    assert_eq!(super::atan2(-0.5, -0.5).to_bits(), 0xc002d97c82ca78cc);
    assert_eq!(super::atan2(0.9, 0.1).to_bits(), 0x3ff75cbae9ec4fce);
    assert_eq!(super::atan2(0.1, 0.9).to_bits(), 0x3fbc5406a57dd4a5);
    assert_eq!(super::atan2(10.0, 1.0).to_bits(), 0x3ff789c03d6f94c5);
    assert_eq!(super::atan2(1.0, 10.0).to_bits(), 0x3fb983b16d49852c);
    assert_eq!(super::atan2(-10.0, 1.0).to_bits(), 0xbff789c03d6f94c5);
    assert_eq!(super::atan2(1.0, -10.0).to_bits(), 0x400855ddc8d9e0ef);
    assert_eq!(super::atan2(1000.0, -0.001).to_bits(), 0x3ff921fc603f3633);
    assert_eq!(super::atan2(-0.001, 1000.0).to_bits(), 0xbeb0bfb091b5fb84);
    assert_eq!(
        super::atan2(0.0, f64::NEG_INFINITY).to_bits(),
        0x7ff8000000000000
    );
    assert_eq!(
        super::atan2(f64::NEG_INFINITY, 0.0).to_bits(),
        0xfff8000000000000
    );
}

#[test]
fn golden_13() {
    // invSqrt/fastInvSqrt/fastInvCubeRoot
    assert_eq!(super::inv_sqrt(0.25f32).to_bits(), 0x40000000);
    assert_eq!(super::inv_sqrt(2.0f32).to_bits(), 0x3f3504f3);
    assert_eq!(super::inv_sqrt(100.0f32).to_bits(), 0x3dcccccd);
    assert_eq!(super::inv_sqrt(1.0f32).to_bits(), 0x3f800000);
    assert_eq!(super::inv_sqrt(4.0f32).to_bits(), 0x3f000000);
    assert_eq!(super::inv_sqrt(3.4028235E38f32).to_bits(), 0x1f800001);
    assert_eq!(super::inv_sqrt(1.4E-45f32).to_bits(), 0x64b504f3);
    assert_eq!(super::inv_sqrt(0.0f32).to_bits(), 0x7f800000);
    assert_eq!(super::inv_sqrt(-1.0f32).to_bits(), 0x7fc00000);
    assert_eq!(super::inv_sqrt(f32::INFINITY).to_bits(), 0x00000000);
    assert_eq!(super::inv_sqrt_f64(0.25).to_bits(), 0x4000000000000000);
    assert_eq!(super::inv_sqrt_f64(2.0).to_bits(), 0x3fe6a09e667f3bcc);
    assert_eq!(super::inv_sqrt_f64(100.0).to_bits(), 0x3fb999999999999a);
    assert_eq!(super::inv_sqrt_f64(1.0).to_bits(), 0x3ff0000000000000);
    assert_eq!(super::inv_sqrt_f64(4.0).to_bits(), 0x3fe0000000000000);
    assert_eq!(
        super::inv_sqrt_f64(1.7976931348623157E308).to_bits(),
        0x1ff0000000000001
    );
    assert_eq!(super::inv_sqrt_f64(4.9E-324).to_bits(), 0x6180000000000000);
    assert_eq!(super::inv_sqrt_f64(0.0).to_bits(), 0x7ff0000000000000);
    assert_eq!(super::inv_sqrt_f64(-1.0).to_bits(), 0x7ff8000000000000);
    assert_eq!(super::inv_sqrt_f64(1.0E-300).to_bits(), 0x5f138d352e5096af);
    assert_eq!(
        super::inv_sqrt_f64(f64::INFINITY).to_bits(),
        0x0000000000000000
    );
    assert_eq!(super::fast_inv_sqrt(0.25).to_bits(), 0x3ffff223eb08e347);
    assert_eq!(super::fast_inv_sqrt(2.0).to_bits(), 0x3fe69f2aee57a7ac);
    assert_eq!(super::fast_inv_sqrt(100.0).to_bits(), 0x3fb98f6d1f8767e4);
    assert_eq!(super::fast_inv_sqrt(1.0).to_bits(), 0x3feff223eb08e347);
    assert_eq!(super::fast_inv_sqrt(4.0).to_bits(), 0x3fdff223eb08e347);
    assert_eq!(
        super::fast_inv_sqrt(1.7976931348623157E308).to_bits(),
        0x1feff223eb08e348
    );
    assert_eq!(super::fast_inv_sqrt(4.9E-324).to_bits(), 0x5ff1307c95c7e9c0);
    assert_eq!(super::fast_inv_sqrt(0.0).to_bits(), 0x5ff1307c95c7e9c0);
    assert_eq!(super::fast_inv_sqrt(-1.0).to_bits(), 0x7ff0000000000000);
    assert_eq!(super::fast_inv_sqrt(1.0E-300).to_bits(), 0x5f1384c08b81fb0b);
    assert_eq!(super::fast_inv_sqrt(1.0E300).to_bits(), 0x20ca26bf40fcf9ae);
    assert_eq!(super::fast_inv_sqrt(0.5).to_bits(), 0x3ff69f2aee57a7ac);
    assert_eq!(super::fast_inv_sqrt(0.1).to_bits(), 0x40094200d5218bb0);
    assert_eq!(
        super::fast_inv_sqrt(12345.678).to_bits(),
        0x3f826ab1bcf1c727
    );
    assert_eq!(super::fast_inv_sqrt(f64::NAN).to_bits(), 0x7ff8000000000000);
    assert_eq!(super::fast_inv_cube_root(1.0f32).to_bits(), 0x3f800008);
    assert_eq!(super::fast_inv_cube_root(8.0f32).to_bits(), 0x3f000008);
    assert_eq!(super::fast_inv_cube_root(27.0f32).to_bits(), 0x3eaaaab7);
    assert_eq!(super::fast_inv_cube_root(0.5f32).to_bits(), 0x3fa14518);
    assert_eq!(super::fast_inv_cube_root(100.0f32).to_bits(), 0x3e5c9d38);
    assert_eq!(super::fast_inv_cube_root(0.0f32).to_bits(), 0x7fc00000);
    assert_eq!(super::fast_inv_cube_root(2.0f32).to_bits(), 0x3f4b2ff6);
    assert_eq!(
        super::fast_inv_cube_root(3.4028235E38f32).to_bits(),
        0x2a214518
    );
    assert_eq!(super::fast_inv_cube_root(1.4E-45f32).to_bits(), 0x5e8c5c5f);
    assert_eq!(super::fast_inv_cube_root(-8.0f32).to_bits(), 0x6910deb6);
    assert_eq!(super::fast_inv_cube_root(-1.0f32).to_bits(), 0x6990deb6);
    assert_eq!(super::fast_inv_cube_root(f32::NAN).to_bits(), 0x7fc00000);
    assert_eq!(
        super::fast_inv_cube_root(f32::INFINITY).to_bits(),
        0x2990deb6
    );
}

#[test]
fn golden_14() {
    // hsvToArgb
    assert_eq!(super::hsv_to_argb(0.0f32, 1.0f32, 1.0f32, 0), 16711680i32);
    assert_eq!(super::hsv_to_argb(0.5f32, 1.0f32, 1.0f32, 0), 65535i32);
    assert_eq!(super::hsv_to_argb(0.33f32, 0.5f32, 0.7f32, 0), 6009433i32);
    assert_eq!(super::hsv_to_argb(1.0f32, 1.0f32, 1.0f32, 0), 16776960i32);
    assert_eq!(super::hsv_to_argb(0.25f32, 1.0f32, 1.0f32, 0), 8388352i32);
    assert_eq!(super::hsv_to_argb(0.75f32, 1.0f32, 1.0f32, 0), 8323327i32);
    assert_eq!(super::hsv_to_argb(0.0f32, 0.0f32, 1.0f32, 0), 16777215i32);
    assert_eq!(super::hsv_to_argb(0.0f32, 1.0f32, 0.0f32, 0), 0i32);
    assert_eq!(
        super::hsv_to_argb(0.08333f32, 1.0f32, 1.0f32, 0),
        16744192i32
    );
    assert_eq!(
        super::hsv_to_argb(0.16666f32, 1.0f32, 1.0f32, 0),
        16776704i32
    );
    assert_eq!(super::hsv_to_argb(0.41666f32, 1.0f32, 1.0f32, 0), 65407i32);
    assert_eq!(super::hsv_to_argb(0.58333f32, 1.0f32, 1.0f32, 0), 32767i32);
    assert_eq!(
        super::hsv_to_argb(0.83333f32, 1.0f32, 1.0f32, 0),
        16646399i32
    );
    assert_eq!(
        super::hsv_to_argb(0.91666f32, 1.0f32, 1.0f32, 0),
        16711807i32
    );
    assert_eq!(super::hsv_to_argb(0.5f32, 0.5f32, 0.5f32, 0), 4161407i32);
    assert_eq!(
        super::hsv_to_argb(0.123f32, 0.456f32, 0.789f32, 0),
        13218157i32
    );
    assert_eq!(super::hsv_to_argb(6.0f32, 1.0f32, 1.0f32, 0), 16776960i32);
    assert_eq!(super::hsv_to_argb(0.0f32, 1.0f32, 1.0f32, 0), 16711680i32);
    assert_eq!(super::hsv_to_argb(0.5, 1.0, 1.0, 128), -2147418113i32);
    assert_eq!(super::hsv_to_argb(0.33, 0.5, 0.7, 255), -10767783i32);
    assert_eq!(super::hsv_to_rgb(0.5, 1.0, 1.0), 65535i32);
}

#[test]
fn golden_15() {
    // misc math
    assert_eq!(
        super::catmullrom(0.5, 0.0, 1.0, 2.0, 3.0).to_bits(),
        0x3fc00000
    );
    assert_eq!(
        super::catmullrom(0.0, 0.0, 1.0, 2.0, 3.0).to_bits(),
        0x3f800000
    );
    assert_eq!(
        super::catmullrom(1.0, 0.0, 1.0, 2.0, 3.0).to_bits(),
        0x40000000
    );
    assert_eq!(super::smoothstep(0.0).to_bits(), 0x0000000000000000);
    assert_eq!(super::smoothstep(0.5).to_bits(), 0x3fe0000000000000);
    assert_eq!(super::smoothstep(1.0).to_bits(), 0x3ff0000000000000);
    assert_eq!(super::smoothstep(-0.5).to_bits(), 0xc003000000000000);
    assert_eq!(super::smoothstep(2.0).to_bits(), 0x4040000000000000);
    assert_eq!(super::smoothstep(f64::NAN).to_bits(), 0x7ff8000000000000);
    assert_eq!(
        super::smoothstep_derivative(0.0).to_bits(),
        0x0000000000000000
    );
    assert_eq!(
        super::smoothstep_derivative(0.5).to_bits(),
        0x3ffe000000000000
    );
    assert_eq!(
        super::smoothstep_derivative(1.0).to_bits(),
        0x0000000000000000
    );
    assert_eq!(
        super::smoothstep_derivative(-0.5).to_bits(),
        0x4030e00000000000
    );
    assert_eq!(
        super::smoothstep_derivative(2.0).to_bits(),
        0x405e000000000000
    );
    assert_eq!(super::sign(0.0), 0i32);
    assert_eq!(super::sign(-0.0), 0i32);
    assert_eq!(super::sign(1.0), 1i32);
    assert_eq!(super::sign(-1.0), -1i32);
    assert_eq!(super::sign(0.5), 1i32);
    assert_eq!(super::sign(-0.5), -1i32);
    assert_eq!(super::sign(f64::NAN), -1i32);
    assert_eq!(super::sign(f64::INFINITY), 1i32);
    assert_eq!(super::rot_lerp(0.5, 10.0, 350.0).to_bits(), 0x00000000);
    assert_eq!(super::rot_lerp(0.5, 350.0, 10.0).to_bits(), 0x43b40000);
    assert_eq!(
        super::rot_lerp_f64(0.5, 10.0, 350.0).to_bits(),
        0x0000000000000000
    );
    assert_eq!(super::rot_lerp_rad(0.5, 0.0, 3.5).to_bits(), 0xbfb21fb6);
    assert_eq!(super::rot_lerp_rad(0.5, 0.0, -3.5).to_bits(), 0x3fb21fb6);
    assert_eq!(
        super::rot_lerp_rad(0.5, 1.0, 1.0 + 6.3).to_bits(),
        0x3f81137e
    );
    assert_eq!(super::triangle_wave(1.0, 4.0).to_bits(), 0x00000000);
    assert_eq!(super::triangle_wave(2.0, 4.0).to_bits(), 0xbf800000);
    assert_eq!(super::triangle_wave(3.0, 4.0).to_bits(), 0x00000000);
    assert_eq!(super::triangle_wave(4.0, 4.0).to_bits(), 0x3f800000);
    assert_eq!(super::triangle_wave(-1.0, 4.0).to_bits(), 0x40000000);
    assert_eq!(super::square_i32(5), 25i32);
    assert_eq!(super::square_i32(-5), 25i32);
    assert_eq!(super::square_i64(3037000499i64), 9223372030926249001i64);
    assert_eq!(super::square_i64(-3037000499i64), 9223372030926249001i64);
    assert_eq!(super::square_f32(1.5).to_bits(), 0x40100000);
    assert_eq!(super::cube(2.0).to_bits(), 0x41000000);
    assert_eq!(super::square_f64(1.5).to_bits(), 0x4002000000000000);
    assert_eq!(
        super::clamped_map(5.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
        0x4062c00000000000
    );
    assert_eq!(
        super::map(5.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
        0x4062c00000000000
    );
    assert_eq!(
        super::inverse_lerp(5.0, 0.0, 10.0).to_bits(),
        0x3fe0000000000000
    );
    assert_eq!(
        super::clamped_map(-5.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
        0x4059000000000000
    );
    assert_eq!(
        super::map(-5.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
        0x4049000000000000
    );
    assert_eq!(
        super::inverse_lerp(-5.0, 0.0, 10.0).to_bits(),
        0xbfe0000000000000
    );
    assert_eq!(
        super::clamped_map(15.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
        0x4069000000000000
    );
    assert_eq!(
        super::map(15.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
        0x406f400000000000
    );
    assert_eq!(
        super::inverse_lerp(15.0, 0.0, 10.0).to_bits(),
        0x3ff8000000000000
    );
    assert_eq!(
        super::clamped_map(5.0, 10.0, 0.0, 100.0, 200.0).to_bits(),
        0x4062c00000000000
    );
    assert_eq!(
        super::map(5.0, 10.0, 0.0, 100.0, 200.0).to_bits(),
        0x4062c00000000000
    );
    assert_eq!(
        super::inverse_lerp(5.0, 10.0, 0.0).to_bits(),
        0x3fe0000000000000
    );
    assert_eq!(
        super::clamped_map_f32(5.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
        0x43160000
    );
    assert_eq!(
        super::map_f32(5.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
        0x43160000
    );
    assert_eq!(
        super::clamped_map_f32(-5.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
        0x42c80000
    );
    assert_eq!(
        super::map_f32(-5.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
        0x42480000
    );
    assert_eq!(
        super::clamped_map_f32(15.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
        0x43480000
    );
    assert_eq!(
        super::map_f32(15.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
        0x437a0000
    );
}

#[test]
fn golden_16() {
    // length/quantize/ceilDiv/roundToward
    assert_eq!(
        super::length_squared(3.0, 4.0).to_bits(),
        0x4039000000000000
    );
    assert_eq!(super::length(3.0, 4.0).to_bits(), 0x4014000000000000);
    assert_eq!(super::length_f32(3.0, 4.0).to_bits(), 0x40a00000);
    assert_eq!(super::length_f32(1.0E20f32, 1.0f32).to_bits(), 0x60ad78ec);
    assert_eq!(
        super::length_f32(1.0000007f32, 1.5000008f32).to_bits(),
        0x3fe6c163
    );
    assert_eq!(
        super::length_squared_xyz(1.0, 2.0, 2.0).to_bits(),
        0x4022000000000000
    );
    assert_eq!(
        super::length_xyz(1.0, 2.0, 2.0).to_bits(),
        0x4008000000000000
    );
    assert_eq!(
        super::length_squared_xyz_f32(1.0, 2.0, 2.0).to_bits(),
        0x41100000
    );
    assert_eq!(super::quantize(7.5, 4), 4i32);
    assert_eq!(super::quantize(-7.5, 4), -8i32);
    assert_eq!(super::quantize(100.0, 16), 96i32);
    assert_eq!(super::quantize(-100.0, 16), -112i32);
    assert_eq!(super::positive_ceil_div(7, 3), 3i32);
    assert_eq!(super::round_toward(7, 3), 9i32);
    assert_eq!(super::positive_ceil_div(-7, 3), -2i32);
    assert_eq!(super::round_toward(-7, 3), -6i32);
    assert_eq!(super::positive_ceil_div(7, -3), -2i32);
    assert_eq!(super::round_toward(7, -3), 6i32);
    assert_eq!(super::positive_ceil_div(-7, -3), 3i32);
    assert_eq!(super::round_toward(-7, -3), -9i32);
    assert_eq!(super::positive_ceil_div(0, 5), 0i32);
    assert_eq!(super::round_toward(0, 5), 0i32);
    assert_eq!(super::positive_ceil_div(-2147483648, 2), 1073741824i32);
    assert_eq!(super::round_toward(-2147483648, 2), -2147483648i32);
    assert_eq!(super::positive_ceil_div(2147483647, 3), 715827883i32);
    assert_eq!(super::round_toward(2147483647, 3), -2147483647i32);
    assert_eq!(super::positive_ceil_div(5, 5), 1i32);
    assert_eq!(super::round_toward(5, 5), 5i32);
    assert_eq!(super::positive_ceil_div(-5, 5), -1i32);
    assert_eq!(super::round_toward(-5, 5), -5i32);
    assert_eq!(super::positive_ceil_div_i64(7i64, 3i64), 3i64);
    assert_eq!(super::round_toward_i64(7i64, 3i64), 9i64);
    assert_eq!(super::positive_ceil_div_i64(-7i64, 3i64), -2i64);
    assert_eq!(super::round_toward_i64(-7i64, 3i64), -6i64);
    assert_eq!(super::positive_ceil_div_i64(7i64, -3i64), -2i64);
    assert_eq!(super::round_toward_i64(7i64, -3i64), 6i64);
    assert_eq!(super::positive_ceil_div_i64(-7i64, -3i64), 3i64);
    assert_eq!(super::round_toward_i64(-7i64, -3i64), -9i64);
    assert_eq!(
        super::positive_ceil_div_i64(-9223372036854775808i64, 2i64),
        4611686018427387904i64
    );
    assert_eq!(
        super::round_toward_i64(-9223372036854775808i64, 2i64),
        -9223372036854775808i64
    );
    assert_eq!(
        super::positive_ceil_div_i64(9223372036854775807i64, 3i64),
        3074457345618258603i64
    );
    assert_eq!(
        super::round_toward_i64(9223372036854775807i64, 3i64),
        -9223372036854775807i64
    );
    assert_eq!(super::positive_ceil_div_i64(5i64, 5i64), 1i64);
    assert_eq!(super::round_toward_i64(5i64, 5i64), 5i64);
}

#[test]
fn golden_17() {
    let mut rNEG9223372036854775808 =
        crate::random_source::SingleThreadedRandomSource::new(-9223372036854775808i64);
    let mut rNEG1 = crate::random_source::SingleThreadedRandomSource::new(-1i64);
    let mut r1 = crate::random_source::SingleThreadedRandomSource::new(1i64);
    let mut r42 = crate::random_source::SingleThreadedRandomSource::new(42i64);
    let mut r123456789 = crate::random_source::SingleThreadedRandomSource::new(123456789i64);
    let mut r244837814047284 =
        crate::random_source::SingleThreadedRandomSource::new(244837814047284i64);
    // LCG nextInt/nextFloat/nextDouble/nextLong/nextBoolean/nextGaussian
    assert_eq!(r1.next_int(), -1155869325i32);
    assert_eq!(r1.next_int(), 431529176i32);
    assert_eq!(r1.next_int(), 1761283695i32);
    assert_eq!(r1.next_int(), 1749940626i32);
    assert_eq!(r1.next_int(), 892128508i32);
    assert_eq!(r1.next_int(), 155629808i32);
    assert_eq!(r1.next_int(), 1429008869i32);
    assert_eq!(r1.next_int(), -1465154083i32);
    assert_eq!(r1.next_int_bound(100), 78i32);
    assert_eq!(r1.next_int_bound(100), 48i32);
    assert_eq!(r1.next_int_bound(100), 69i32);
    assert_eq!(r1.next_int_bound(100), 73i32);
    assert_eq!(r1.next_int_bound(100), 17i32);
    assert_eq!(r1.next_int_bound(5), 3i32);
    assert_eq!(r1.next_int_bound(5), 2i32);
    assert_eq!(r1.next_int_bound(5), 4i32);
    assert_eq!(r1.next_int_bound(5), 2i32);
    assert_eq!(r1.next_int_bound(5), 2i32);
    assert_eq!(r1.next_float().to_bits(), 0x3f6fe49d);
    assert_eq!(r1.next_float().to_bits(), 0x3ef96384);
    assert_eq!(r1.next_float().to_bits(), 0x3ecb5a6e);
    assert_eq!(r1.next_float().to_bits(), 0x3f69f978);
    assert_eq!(r1.next_float().to_bits(), 0x3eb1ede2);
    assert_eq!(r1.next_double().to_bits(), 0x3fc4653d00000000);
    assert_eq!(r1.next_double().to_bits(), 0x3fd79dbce0000000);
    assert_eq!(r1.next_double().to_bits(), 0x3feba9ae40000000);
    assert_eq!(r1.next_double().to_bits(), 0x3fe1419d80000000);
    assert_eq!(r1.next_double().to_bits(), 0x3fe2aad5c0000000);
    assert_eq!(r1.next_long(), 780266760877150279i64);
    assert_eq!(r1.next_long(), -4396325885451314877i64);
    assert_eq!(r1.next_long(), -6936482374120670025i64);
    assert_eq!(r1.next_long(), -2227205230235739057i64);
    assert_eq!(r1.next_long(), 8305063674006169674i64);
    assert_eq!(r1.next_boolean(), false);
    assert_eq!(r1.next_boolean(), false);
    assert_eq!(r1.next_boolean(), true);
    assert_eq!(r1.next_boolean(), true);
    assert_eq!(r1.next_boolean(), false);
    assert_eq!(r1.next_gaussian().to_bits(), 0x3fdb4bc679e68cf4);
    assert_eq!(r1.next_gaussian().to_bits(), 0xbfe406477a919a7f);
    assert_eq!(r1.next_gaussian().to_bits(), 0xbff292e9cfcec7de);
    assert_eq!(r1.next_gaussian().to_bits(), 0x4006a55727d633c9);
    assert_eq!(r1.next_gaussian().to_bits(), 0x3fd6bffeb657b36f);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), 0x3ffe0979c8000000);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), 0x3fe660850b000000);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), 0x3fd19ec540000000);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), 0x4003a78e9c000000);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), 0x3ff5a95198000000);
    assert_eq!(r1.next_int_between_inclusive(10, 20), 18i32);
    assert_eq!(r1.next_int_between_inclusive(10, 20), 20i32);
    assert_eq!(r1.next_int_between_inclusive(10, 20), 20i32);
    assert_eq!(r42.next_int(), -1170105035i32);
    assert_eq!(r42.next_int(), 234785527i32);
    assert_eq!(r42.next_int(), -1360544799i32);
    assert_eq!(r42.next_int(), 205897768i32);
    assert_eq!(r42.next_int(), 1325939940i32);
    assert_eq!(r42.next_int(), -248792245i32);
    assert_eq!(r42.next_int(), 1190043011i32);
    assert_eq!(r42.next_int(), -1255373459i32);
    assert_eq!(r42.next_int_bound(100), 19i32);
    assert_eq!(r42.next_int_bound(100), 93i32);
    assert_eq!(r42.next_int_bound(100), 82i32);
    assert_eq!(r42.next_int_bound(100), 2i32);
    assert_eq!(r42.next_int_bound(100), 76i32);
    assert_eq!(r42.next_int_bound(5), 2i32);
    assert_eq!(r42.next_int_bound(5), 1i32);
    assert_eq!(r42.next_int_bound(5), 2i32);
    assert_eq!(r42.next_int_bound(5), 1i32);
    assert_eq!(r42.next_int_bound(5), 0i32);
    assert_eq!(r42.next_float().to_bits(), 0x3f486c40);
    assert_eq!(r42.next_float().to_bits(), 0x3f7f88a1);
    assert_eq!(r42.next_float().to_bits(), 0x3f6b5910);
    assert_eq!(r42.next_float().to_bits(), 0x3e1b9af0);
    assert_eq!(r42.next_float().to_bits(), 0x3edf7bbe);
    assert_eq!(r42.next_double().to_bits(), 0x3fdc25ae20000000);
    assert_eq!(r42.next_double().to_bits(), 0x3fedc7bb00000000);
    assert_eq!(r42.next_double().to_bits(), 0x3fe98b9060000000);
    assert_eq!(r42.next_double().to_bits(), 0x3fc34523e0000000);
    assert_eq!(r42.next_double().to_bits(), 0x3fd5a6d660000000);
    assert_eq!(r42.next_long(), 4623885210859241458i64);
    assert_eq!(r42.next_long(), 6736035388467870105i64);
    assert_eq!(r42.next_long(), 2926660232063067186i64);
    assert_eq!(r42.next_long(), 5057591690988349847i64);
    assert_eq!(r42.next_long(), -6574010802050714786i64);
    assert_eq!(r42.next_boolean(), false);
    assert_eq!(r42.next_boolean(), true);
    assert_eq!(r42.next_boolean(), false);
    assert_eq!(r42.next_boolean(), true);
    assert_eq!(r42.next_boolean(), true);
    assert_eq!(r42.next_gaussian().to_bits(), 0xbfd1b79c94791c2a);
    assert_eq!(r42.next_gaussian().to_bits(), 0xbfb57d066f4ea165);
    assert_eq!(r42.next_gaussian().to_bits(), 0x3ff417e442ae6000);
    assert_eq!(r42.next_gaussian().to_bits(), 0xbfd4d14503bb4c01);
    assert_eq!(r42.next_gaussian().to_bits(), 0xbfc62e607fda46c2);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), 0x3ff423c900000000);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), 0x3ff88f8c80000000);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), 0x3feb3cedc0000000);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), 0x3fef042800000000);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), 0x3fd2233a80000000);
    assert_eq!(r42.next_int_between_inclusive(10, 20), 12i32);
    assert_eq!(r42.next_int_between_inclusive(10, 20), 12i32);
    assert_eq!(r42.next_int_between_inclusive(10, 20), 18i32);
    assert_eq!(r123456789.next_int(), -1442945365i32);
    assert_eq!(r123456789.next_int(), -1016548095i32);
    assert_eq!(r123456789.next_int(), 1962592967i32);
    assert_eq!(r123456789.next_int(), 1094656688i32);
    assert_eq!(r123456789.next_int(), 1677212580i32);
    assert_eq!(r123456789.next_int(), 930275108i32);
    assert_eq!(r123456789.next_int(), -458096230i32);
    assert_eq!(r123456789.next_int(), 1827465615i32);
    assert_eq!(r123456789.next_int_bound(100), 97i32);
    assert_eq!(r123456789.next_int_bound(100), 87i32);
    assert_eq!(r123456789.next_int_bound(100), 41i32);
    assert_eq!(r123456789.next_int_bound(100), 61i32);
    assert_eq!(r123456789.next_int_bound(100), 64i32);
    assert_eq!(r123456789.next_int_bound(5), 2i32);
    assert_eq!(r123456789.next_int_bound(5), 4i32);
    assert_eq!(r123456789.next_int_bound(5), 1i32);
    assert_eq!(r123456789.next_int_bound(5), 0i32);
    assert_eq!(r123456789.next_int_bound(5), 4i32);
    assert_eq!(r123456789.next_float().to_bits(), 0x3e7ecd30);
    assert_eq!(r123456789.next_float().to_bits(), 0x3f22205f);
    assert_eq!(r123456789.next_float().to_bits(), 0x3e8c793c);
    assert_eq!(r123456789.next_float().to_bits(), 0x3f002291);
    assert_eq!(r123456789.next_float().to_bits(), 0x3f105f17);
    assert_eq!(r123456789.next_double().to_bits(), 0x3fda256d20000000);
    assert_eq!(r123456789.next_double().to_bits(), 0x3fee54c2e0000000);
    assert_eq!(r123456789.next_double().to_bits(), 0x3fe3155a60000000);
    assert_eq!(r123456789.next_double().to_bits(), 0x3fd12f08e0000000);
    assert_eq!(r123456789.next_double().to_bits(), 0x3fdefc4fc0000000);
    assert_eq!(r123456789.next_long(), -9218142432141340534i64);
    assert_eq!(r123456789.next_long(), 8938299083908680542i64);
    assert_eq!(r123456789.next_long(), 2269987056764843472i64);
    assert_eq!(r123456789.next_long(), -2395981303671479804i64);
    assert_eq!(r123456789.next_long(), 1542852440207316228i64);
    assert_eq!(r123456789.next_boolean(), true);
    assert_eq!(r123456789.next_boolean(), true);
    assert_eq!(r123456789.next_boolean(), true);
    assert_eq!(r123456789.next_boolean(), false);
    assert_eq!(r123456789.next_boolean(), true);
    assert_eq!(r123456789.next_gaussian().to_bits(), 0x3ffaa1a40863febb);
    assert_eq!(r123456789.next_gaussian().to_bits(), 0x3fe33c4214958786);
    assert_eq!(r123456789.next_gaussian().to_bits(), 0xbffe23ba6997f903);
    assert_eq!(r123456789.next_gaussian().to_bits(), 0xbfd927895784aa68);
    assert_eq!(r123456789.next_gaussian().to_bits(), 0xbfff34e2d83a0b91);
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
        0x4005664650000000
    );
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
        0x3fefccd480000000
    );
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
        0x3ff5fb6e30000000
    );
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
        0x3fef8fa280000000
    );
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
        0xbfdb97cfc0000000
    );
    assert_eq!(r123456789.next_int_between_inclusive(10, 20), 14i32);
    assert_eq!(r123456789.next_int_between_inclusive(10, 20), 15i32);
    assert_eq!(r123456789.next_int_between_inclusive(10, 20), 20i32);
    assert_eq!(rNEG1.next_int(), 1155099827i32);
    assert_eq!(rNEG1.next_int(), 1887904451i32);
    assert_eq!(rNEG1.next_int(), 52699159i32);
    assert_eq!(rNEG1.next_int(), -1941176418i32);
    assert_eq!(rNEG1.next_int(), -1451336087i32);
    assert_eq!(rNEG1.next_int(), -1714570420i32);
    assert_eq!(rNEG1.next_int(), 1788588954i32);
    assert_eq!(rNEG1.next_int(), 1714930956i32);
    assert_eq!(rNEG1.next_int_bound(100), 65i32);
    assert_eq!(rNEG1.next_int_bound(100), 31i32);
    assert_eq!(rNEG1.next_int_bound(100), 8i32);
    assert_eq!(rNEG1.next_int_bound(100), 12i32);
    assert_eq!(rNEG1.next_int_bound(100), 17i32);
    assert_eq!(rNEG1.next_int_bound(5), 1i32);
    assert_eq!(rNEG1.next_int_bound(5), 2i32);
    assert_eq!(rNEG1.next_int_bound(5), 0i32);
    assert_eq!(rNEG1.next_int_bound(5), 4i32);
    assert_eq!(rNEG1.next_int_bound(5), 2i32);
    assert_eq!(rNEG1.next_float().to_bits(), 0x3f0f2e80);
    assert_eq!(rNEG1.next_float().to_bits(), 0x3f3b456d);
    assert_eq!(rNEG1.next_float().to_bits(), 0x3f196703);
    assert_eq!(rNEG1.next_float().to_bits(), 0x3eb3d118);
    assert_eq!(rNEG1.next_float().to_bits(), 0x3f6e2493);
    assert_eq!(rNEG1.next_double().to_bits(), 0x3fdd127000000000);
    assert_eq!(rNEG1.next_double().to_bits(), 0x3fd578b9e0000000);
    assert_eq!(rNEG1.next_double().to_bits(), 0x3f67e6c960000000);
    assert_eq!(rNEG1.next_double().to_bits(), 0x3fc35fb0a0000000);
    assert_eq!(rNEG1.next_double().to_bits(), 0x3fe6368aa0000000);
    assert_eq!(rNEG1.next_long(), 1946119930767781179i64);
    assert_eq!(rNEG1.next_long(), -5598838322766633025i64);
    assert_eq!(rNEG1.next_long(), 6472470557549166806i64);
    assert_eq!(rNEG1.next_long(), 5307374797520035014i64);
    assert_eq!(rNEG1.next_long(), 1590261952932679623i64);
    assert_eq!(rNEG1.next_boolean(), true);
    assert_eq!(rNEG1.next_boolean(), true);
    assert_eq!(rNEG1.next_boolean(), false);
    assert_eq!(rNEG1.next_boolean(), false);
    assert_eq!(rNEG1.next_boolean(), true);
    assert_eq!(rNEG1.next_gaussian().to_bits(), 0xbfe16df531b2d58f);
    assert_eq!(rNEG1.next_gaussian().to_bits(), 0x3fe0311a5a30f323);
    assert_eq!(rNEG1.next_gaussian().to_bits(), 0xbff6b70d3db41fbc);
    assert_eq!(rNEG1.next_gaussian().to_bits(), 0xbff9a80cd6c66a19);
    assert_eq!(rNEG1.next_gaussian().to_bits(), 0xbfd60f65dc9659fc);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), 0x3fe55f9330000000);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), 0x3ff4a1bac0000000);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), 0x4005442892800000);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), 0x40034fbe50000000);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), 0x40022c0084000000);
    assert_eq!(rNEG1.next_int_between_inclusive(10, 20), 19i32);
    assert_eq!(rNEG1.next_int_between_inclusive(10, 20), 19i32);
    assert_eq!(rNEG1.next_int_between_inclusive(10, 20), 11i32);
    assert_eq!(rNEG9223372036854775808.next_int(), -1155484576i32);
    assert_eq!(rNEG9223372036854775808.next_int(), -723955400i32);
    assert_eq!(rNEG9223372036854775808.next_int(), 1033096058i32);
    assert_eq!(rNEG9223372036854775808.next_int(), -1690734402i32);
    assert_eq!(rNEG9223372036854775808.next_int(), -1557280266i32);
    assert_eq!(rNEG9223372036854775808.next_int(), 1327362106i32);
    assert_eq!(rNEG9223372036854775808.next_int(), -1930858313i32);
    assert_eq!(rNEG9223372036854775808.next_int(), 502539523i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), 19i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), 54i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), 77i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), 77i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), 73i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), 2i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), 0i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), 4i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), 4i32);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), 0i32);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), 0x3f70f5b4);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), 0x3e343340);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), 0x3e8cc6c4);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), 0x3ea03924);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), 0x3e03fd9c);
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
        0x3fd78cea40000000
    );
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
        0x3fe690caa0000000
    );
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
        0x3f56b828c0000000
    );
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
        0x3f82e47be0000000
    );
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
        0x3fe203af00000000
    );
    assert_eq!(rNEG9223372036854775808.next_long(), 4664870386390374308i64);
    assert_eq!(rNEG9223372036854775808.next_long(), 5274012497411174555i64);
    assert_eq!(rNEG9223372036854775808.next_long(), 275355961613117157i64);
    assert_eq!(rNEG9223372036854775808.next_long(), -429849028627267071i64);
    assert_eq!(rNEG9223372036854775808.next_long(), -5751058041584006506i64);
    assert_eq!(rNEG9223372036854775808.next_boolean(), true);
    assert_eq!(rNEG9223372036854775808.next_boolean(), true);
    assert_eq!(rNEG9223372036854775808.next_boolean(), true);
    assert_eq!(rNEG9223372036854775808.next_boolean(), true);
    assert_eq!(rNEG9223372036854775808.next_boolean(), true);
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
        0x3fd7d4517f1180e5
    );
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
        0x3fd97377da512484
    );
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
        0x3fb01d3638e3d2fe
    );
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
        0x3fee20708fc3e4bd
    );
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
        0x3fdc3b0b51c9c8c3
    );
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
        0x3ffeabcdf2c00000
    );
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
        0x3fdc8dd290000000
    );
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
        0xbfa514ba00000000
    );
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
        0x3ff362b900000000
    );
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
        0x3ffddffec0000000
    );
    assert_eq!(
        rNEG9223372036854775808.next_int_between_inclusive(10, 20),
        17i32
    );
    assert_eq!(
        rNEG9223372036854775808.next_int_between_inclusive(10, 20),
        13i32
    );
    assert_eq!(
        rNEG9223372036854775808.next_int_between_inclusive(10, 20),
        14i32
    );
    assert_eq!(r244837814047284.next_int(), -885268670i32);
    assert_eq!(r244837814047284.next_int(), 1048203960i32);
    assert_eq!(r244837814047284.next_int(), -711164248i32);
    assert_eq!(r244837814047284.next_int(), 2029955585i32);
    assert_eq!(r244837814047284.next_int(), 970502513i32);
    assert_eq!(r244837814047284.next_int(), -299354617i32);
    assert_eq!(r244837814047284.next_int(), -2065049096i32);
    assert_eq!(r244837814047284.next_int(), 2137960893i32);
    assert_eq!(r244837814047284.next_int_bound(100), 93i32);
    assert_eq!(r244837814047284.next_int_bound(100), 84i32);
    assert_eq!(r244837814047284.next_int_bound(100), 39i32);
    assert_eq!(r244837814047284.next_int_bound(100), 46i32);
    assert_eq!(r244837814047284.next_int_bound(100), 64i32);
    assert_eq!(r244837814047284.next_int_bound(5), 0i32);
    assert_eq!(r244837814047284.next_int_bound(5), 3i32);
    assert_eq!(r244837814047284.next_int_bound(5), 2i32);
    assert_eq!(r244837814047284.next_int_bound(5), 4i32);
    assert_eq!(r244837814047284.next_int_bound(5), 2i32);
    assert_eq!(r244837814047284.next_float().to_bits(), 0x3f00dd9e);
    assert_eq!(r244837814047284.next_float().to_bits(), 0x3f274dad);
    assert_eq!(r244837814047284.next_float().to_bits(), 0x3ddb3370);
    assert_eq!(r244837814047284.next_float().to_bits(), 0x3f78edbe);
    assert_eq!(r244837814047284.next_float().to_bits(), 0x3f7f0bfc);
    assert_eq!(r244837814047284.next_double().to_bits(), 0x3f9dfa5760000000);
    assert_eq!(r244837814047284.next_double().to_bits(), 0x3fec8bcdc0000000);
    assert_eq!(r244837814047284.next_double().to_bits(), 0x3fd24ad880000000);
    assert_eq!(r244837814047284.next_double().to_bits(), 0x3fee3a07a0000000);
    assert_eq!(r244837814047284.next_double().to_bits(), 0x3fd87c9b60000000);
    assert_eq!(r244837814047284.next_long(), -8658503656214245666i64);
    assert_eq!(r244837814047284.next_long(), 7722813782141551780i64);
    assert_eq!(r244837814047284.next_long(), 5968427110623386406i64);
    assert_eq!(r244837814047284.next_long(), 5270872859802110971i64);
    assert_eq!(r244837814047284.next_long(), -7012647755091709195i64);
    assert_eq!(r244837814047284.next_boolean(), true);
    assert_eq!(r244837814047284.next_boolean(), false);
    assert_eq!(r244837814047284.next_boolean(), true);
    assert_eq!(r244837814047284.next_boolean(), true);
    assert_eq!(r244837814047284.next_boolean(), false);
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
        0x3fea0dc18be1aac5
    );
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
        0xbfeb326113ef8922
    );
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
        0x3ff0950dee23f71f
    );
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
        0x3ffe263181e013a0
    );
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
        0x3fd7bdbc5c56242f
    );
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
        0xbfe5dc9ef0000000
    );
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
        0xbfd2f38108000000
    );
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
        0x4000e733e0000000
    );
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
        0x3fe9d70880000000
    );
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
        0x3fda0adf00000000
    );
    assert_eq!(r244837814047284.next_int_between_inclusive(10, 20), 13i32);
    assert_eq!(r244837814047284.next_int_between_inclusive(10, 20), 11i32);
    assert_eq!(r244837814047284.next_int_between_inclusive(10, 20), 16i32);
}

#[test]
fn golden_18() {
    let mut r0 = crate::random_source::SingleThreadedRandomSource::new(0i64);
    let mut r1 = crate::random_source::SingleThreadedRandomSource::new(1i64);
    let mut r7 = crate::random_source::SingleThreadedRandomSource::new(7i64);
    let mut r999 = crate::random_source::SingleThreadedRandomSource::new(999i64);
    // Mth RNG helpers
    assert_eq!(super::next_int(&mut r0, 3, 10), 8i32);
    assert_eq!(super::next_float(&mut r0, 1.0, 2.0).to_bits(), 0x3fea6ca8);
    assert_eq!(
        super::next_double(&mut r0, 1.0, 2.0).to_bits(),
        0x3ff3d93cb8000000
    );
    assert_eq!(super::random_between_inclusive(&mut r0, 10, 30), 21i32);
    assert_eq!(
        super::random_between(&mut r0, 5.0, 10.0).to_bits(),
        0x40d172b6
    );
    assert_eq!(super::normal(&mut r0, 100.0, 5.0).to_bits(), 0x42d34f45);
    assert_eq!(super::next_int(&mut r1, 3, 10), 8i32);
    assert_eq!(super::next_float(&mut r1, 1.0, 2.0).to_bits(), 0x3f8cdc4e);
    assert_eq!(
        super::next_double(&mut r1, 1.0, 2.0).to_bits(),
        0x3ff68fb0e8000000
    );
    assert_eq!(super::random_between_inclusive(&mut r1, 10, 30), 30i32);
    assert_eq!(
        super::random_between(&mut r1, 5.0, 10.0).to_bits(),
        0x40a5cc33
    );
    assert_eq!(super::normal(&mut r1, 100.0, 5.0).to_bits(), 0x42c7752c);
    assert_eq!(super::next_int(&mut r7, 3, 10), 8i32);
    assert_eq!(super::next_float(&mut r7, 1.0, 2.0).to_bits(), 0x3fd1bb9a);
    assert_eq!(
        super::next_double(&mut r7, 1.0, 2.0).to_bits(),
        0x3ffbfc9940000000
    );
    assert_eq!(super::random_between_inclusive(&mut r7, 10, 30), 29i32);
    assert_eq!(
        super::random_between(&mut r7, 5.0, 10.0).to_bits(),
        0x40ee7f2e
    );
    assert_eq!(super::normal(&mut r7, 100.0, 5.0).to_bits(), 0x42cdd6f3);
    assert_eq!(super::next_int(&mut r999, 3, 10), 8i32);
    assert_eq!(super::next_float(&mut r999, 1.0, 2.0).to_bits(), 0x3ff36cb0);
    assert_eq!(
        super::next_double(&mut r999, 1.0, 2.0).to_bits(),
        0x3ffba242b0000000
    );
    assert_eq!(super::random_between_inclusive(&mut r999, 10, 30), 18i32);
    assert_eq!(
        super::random_between(&mut r999, 5.0, 10.0).to_bits(),
        0x40dcccf2
    );
    assert_eq!(super::normal(&mut r999, 100.0, 5.0).to_bits(), 0x42beeac0);
}

#[test]
fn golden_19() {
    // wobble / createInsecureUUID
    assert_eq!(super::wobble(1.0).to_bits(), 0x3ff000000ac25f71);
    assert_eq!(super::wobble(0.0).to_bits(), 0x3e58cccb1f4da321);
    assert_eq!(super::wobble(-1.0).to_bits(), 0xbff000000ac610f8);
    assert_eq!(super::wobble(123.456).to_bits(), 0x405edd2f1a8c2473);
    assert_eq!(super::wobble(-987.654).to_bits(), 0xc08edd3b645dea5b);
    assert_eq!(super::wobble(3.0E-4).to_bits(), 0x3f33a98d6381af98);
    assert_eq!(super::wobble(-3.0E-4).to_bits(), 0xbf33a98d6d5c23ed);
    assert_eq!(
        super::create_insecure_uuid(&mut crate::random_source::SingleThreadedRandomSource::new(
            1i64
        ))
        .most,
        -4964420948893086504i64
    );
    assert_eq!(
        super::create_insecure_uuid(&mut crate::random_source::SingleThreadedRandomSource::new(
            1i64
        ))
        .least,
        -6270402184529184366i64
    );
    assert_eq!(
        super::create_insecure_uuid(&mut crate::random_source::SingleThreadedRandomSource::new(
            42i64
        ))
        .most,
        -5025562857975166217i64
    );
    assert_eq!(
        super::create_insecure_uuid(&mut crate::random_source::SingleThreadedRandomSource::new(
            42i64
        ))
        .least,
        -5843495416241995736i64
    );
}

#[test]
fn golden_20() {
    // binarySearch
    assert_eq!(super::binary_search(0, 10, |x| x * x > 50), 8i32);
    assert_eq!(super::binary_search(0, 10, |x| x >= 3), 3i32);
    assert_eq!(super::binary_search(0, 10, |x| x > 100), 10i32);
    assert_eq!(super::binary_search(0, 0, |x| x >= 0), 0i32);
    assert_eq!(super::binary_search(-10, 10, |x| x >= 0), 0i32);
}

#[test]
fn golden_21() {
    // outFromOrigin — expected values from OpenJDK `IntStream.iterate`
    // (verified by running the Java Mth.outFromOrigin). The seed and every
    // subsequent element are gated through the `hasNext` predicate, matching
    // OpenJDK; the earlier off-by-one (a trailing element) is fixed, so these
    // expected vectors are authoritative.
    assert_eq!(
        super::out_from_origin(0, -3, 3).collect::<Vec<_>>(),
        vec![0, 1, -1, 2, -2, 3, -3]
    );
    assert_eq!(
        super::out_from_origin(0, 0, 5).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        super::out_from_origin(10, 0, 20).collect::<Vec<_>>(),
        vec![
            10, 11, 9, 12, 8, 13, 7, 14, 6, 15, 5, 16, 4, 17, 3, 18, 2, 19, 1, 20, 0
        ]
    );
    assert_eq!(
        super::out_from_origin(5, 0, 5).collect::<Vec<_>>(),
        vec![5, 4, 3, 2, 1, 0]
    );
    assert_eq!(
        super::out_from_origin(-5, 0, 10).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    );
    assert_eq!(
        super::out_from_origin(20, 0, 10).collect::<Vec<_>>(),
        vec![10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]
    );
    assert_eq!(
        super::out_from_origin(0, 0, 1).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(super::out_from_origin(7, 7, 7).collect::<Vec<_>>(), vec![7]);
    assert_eq!(
        super::out_from_origin(0, -10, -5).collect::<Vec<_>>(),
        vec![-5, -6, -7, -8, -9, -10]
    );
    assert_eq!(
        super::out_from_origin(3, 3, 9).collect::<Vec<_>>(),
        vec![3, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(
        super::out_from_origin_with_step(0, -5, 5, 2).collect::<Vec<_>>(),
        vec![0, 2, -2, 4, -4]
    );
    assert_eq!(
        super::out_from_origin_with_step(0, 0, 9, 3).collect::<Vec<_>>(),
        vec![0, 3, 6, 9]
    );
    assert_eq!(
        super::out_from_origin_with_step(5, 0, 20, 4).collect::<Vec<_>>(),
        vec![5, 9, 1, 13, 17]
    );
    assert_eq!(
        super::out_from_origin_with_step(0, 0, 5, 1).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        super::out_from_origin_with_step(2, 0, 20, 2).collect::<Vec<_>>(),
        vec![2, 4, 0, 6, 8, 10, 12, 14, 16, 18, 20]
    );
}

#[test]
fn golden_22() {
    // rayIntersectsAABB / rotationAroundAxis
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(1.0, 1.0, 1.0),
            crate::mth_stubs::Vec3::new(0.0, 0.0, 1.0),
            crate::mth_stubs::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 2.0,
                max_y: 2.0,
                max_z: 2.0
            }
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(0.0, 0.0, 0.0),
            crate::mth_stubs::Vec3::new(1.0, 0.0, 0.0),
            crate::mth_stubs::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 2.0,
                max_y: 2.0,
                max_z: 2.0
            }
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(5.0, 5.0, 5.0),
            crate::mth_stubs::Vec3::new(0.0, 0.0, 0.0),
            crate::mth_stubs::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 2.0,
                max_y: 2.0,
                max_z: 2.0
            }
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(1.0, 1.0, 1.0),
            crate::mth_stubs::Vec3::new(0.0, 0.0, -1.0),
            crate::mth_stubs::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 2.0,
                max_y: 2.0,
                max_z: 2.0
            }
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(-1.0, -1.0, -1.0),
            crate::mth_stubs::Vec3::new(1.0, 1.0, 1.0),
            crate::mth_stubs::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 2.0,
                max_y: 2.0,
                max_z: 2.0
            }
        ),
        true
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(0.0, 0.0, 0.0),
            crate::mth_stubs::Vec3::new(-1.0, -1.0, -1.0),
            crate::mth_stubs::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 2.0,
                max_y: 2.0,
                max_z: 2.0
            }
        ),
        true
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(2.0, 2.0, 2.0),
            crate::mth_stubs::Vec3::new(1.0, 0.0, 0.0),
            crate::mth_stubs::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 2.0,
                max_y: 2.0,
                max_z: 2.0
            }
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(0.0, 0.0, 0.0),
            crate::mth_stubs::Vec3::new(0.0, 0.0, 1.0),
            crate::mth_stubs::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                max_x: 2.0,
                max_y: 2.0,
                max_z: 2.0
            }
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(1.0, 1.0, 1.0),
            crate::mth_stubs::Vec3::new(1.0, 1.0, 1.0),
            crate::mth_stubs::Aabb {
                min_x: 1.0,
                min_y: 1.0,
                min_z: 1.0,
                max_x: 3.0,
                max_y: 3.0,
                max_z: 3.0
            }
        ),
        true
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(1.0, 1.0, 1.0),
            crate::mth_stubs::Vec3::new(0.0, 0.0, -1.0),
            crate::mth_stubs::Aabb {
                min_x: 1.0,
                min_y: 1.0,
                min_z: 1.0,
                max_x: 3.0,
                max_y: 3.0,
                max_z: 3.0
            }
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(3.0, 3.0, 3.0),
            crate::mth_stubs::Vec3::new(-1.0, -1.0, -1.0),
            crate::mth_stubs::Aabb {
                min_x: 1.0,
                min_y: 1.0,
                min_z: 1.0,
                max_x: 3.0,
                max_y: 3.0,
                max_z: 3.0
            }
        ),
        true
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::mth_stubs::Vec3::new(0.5, 0.5, 0.5),
            crate::mth_stubs::Vec3::new(1.0, 0.0, 0.0),
            crate::mth_stubs::Aabb {
                min_x: 1.0,
                min_y: 1.0,
                min_z: 1.0,
                max_x: 3.0,
                max_y: 3.0,
                max_z: 3.0
            }
        ),
        false
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 1.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.3f32,
                y: 0.4f32,
                z: 0.5f32,
                w: 0.6f32
            }
        )
        .x
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 1.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.3f32,
                y: 0.4f32,
                z: 0.5f32,
                w: 0.6f32
            }
        )
        .y
        .to_bits(),
        0x3f0e00d5
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 1.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.3f32,
                y: 0.4f32,
                z: 0.5f32,
                w: 0.6f32
            }
        )
        .z
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 1.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.3f32,
                y: 0.4f32,
                z: 0.5f32,
                w: 0.6f32
            }
        )
        .w
        .to_bits(),
        0x3f550140
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 1.0f32,
                y: 0.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.1f32,
                y: 0.2f32,
                z: 0.3f32,
                w: 0.4f32
            }
        )
        .x
        .to_bits(),
        0x3e785b42
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 1.0f32,
                y: 0.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.1f32,
                y: 0.2f32,
                z: 0.3f32,
                w: 0.4f32
            }
        )
        .y
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 1.0f32,
                y: 0.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.1f32,
                y: 0.2f32,
                z: 0.3f32,
                w: 0.4f32
            }
        )
        .z
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 1.0f32,
                y: 0.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.1f32,
                y: 0.2f32,
                z: 0.3f32,
                w: 0.4f32
            }
        )
        .w
        .to_bits(),
        0x3f785b42
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 0.0f32,
                z: 1.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.9f32,
                y: 0.8f32,
                z: 0.7f32,
                w: 0.6f32
            }
        )
        .x
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 0.0f32,
                z: 1.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.9f32,
                y: 0.8f32,
                z: 0.7f32,
                w: 0.6f32
            }
        )
        .y
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 0.0f32,
                z: 1.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.9f32,
                y: 0.8f32,
                z: 0.7f32,
                w: 0.6f32
            }
        )
        .z
        .to_bits(),
        0x3f425ea4
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 0.0f32,
                z: 1.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.9f32,
                y: 0.8f32,
                z: 0.7f32,
                w: 0.6f32
            }
        )
        .w
        .to_bits(),
        0x3f269a44
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 1.0f32,
                y: 1.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.0f32,
                y: 0.0f32,
                z: 0.0f32,
                w: 1.0f32
            }
        )
        .x
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 1.0f32,
                y: 1.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.0f32,
                y: 0.0f32,
                z: 0.0f32,
                w: 1.0f32
            }
        )
        .y
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 1.0f32,
                y: 1.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.0f32,
                y: 0.0f32,
                z: 0.0f32,
                w: 1.0f32
            }
        )
        .z
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 1.0f32,
                y: 1.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.0f32,
                y: 0.0f32,
                z: 0.0f32,
                w: 1.0f32
            }
        )
        .w
        .to_bits(),
        0x3f800000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 0.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.5f32,
                y: 0.5f32,
                z: 0.5f32,
                w: 0.5f32
            }
        )
        .x
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 0.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.5f32,
                y: 0.5f32,
                z: 0.5f32,
                w: 0.5f32
            }
        )
        .y
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 0.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.5f32,
                y: 0.5f32,
                z: 0.5f32,
                w: 0.5f32
            }
        )
        .z
        .to_bits(),
        0x00000000
    );
    assert_eq!(
        super::rotation_around_axis(
            crate::mth_stubs::Vec3f {
                x: 0.0f32,
                y: 0.0f32,
                z: 0.0f32
            },
            crate::mth_stubs::Quaternionf {
                x: 0.5f32,
                y: 0.5f32,
                z: 0.5f32,
                w: 0.5f32
            }
        )
        .w
        .to_bits(),
        0x3f800000
    );
}

#[test]
fn golden_23() {
    // mulAndTruncate
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 3,
                denominator: 4
            },
            100
        ),
        75i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 1,
                denominator: 2
            },
            100
        ),
        50i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 5,
                denominator: 3
            },
            100
        ),
        166i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 7,
                denominator: 1
            },
            100
        ),
        700i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 1,
                denominator: 3
            },
            100
        ),
        33i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 2147483647,
                denominator: 2
            },
            100
        ),
        -50i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 3,
                denominator: 4
            },
            7
        ),
        5i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 1,
                denominator: 2
            },
            7
        ),
        3i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 5,
                denominator: 3
            },
            7
        ),
        11i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 7,
                denominator: 1
            },
            7
        ),
        49i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 1,
                denominator: 3
            },
            7
        ),
        2i32
    );
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 2147483647,
                denominator: 2
            },
            7
        ),
        1073741820i32
    );
}
