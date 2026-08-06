// GENERATED golden tests vs Java Mth oracle (working/Paper expressions). Do not hand-edit.
#![allow(clippy::all)]
#![allow(non_snake_case)]
use crate::random::RandomSource;
#[test]
fn golden_0() {
    // sin/cos
    assert_eq!(super::sin(0.0).to_bits(), @@0@@);
    assert_eq!(super::sin(1.0).to_bits(), @@1@@);
    assert_eq!(super::sin(2.0).to_bits(), @@2@@);
    assert_eq!(super::sin(3.0).to_bits(), @@3@@);
    assert_eq!(super::sin(0.5).to_bits(), @@4@@);
    assert_eq!(super::sin(3.141592653589793).to_bits(), @@5@@);
    assert_eq!(super::sin(1.5707963267948966).to_bits(), @@6@@);
    assert_eq!(super::sin(6.283185307179586).to_bits(), @@7@@);
    assert_eq!(super::sin(-3.141592653589793).to_bits(), @@8@@);
    assert_eq!(super::sin(-1.5707963267948966).to_bits(), @@9@@);
    assert_eq!(super::sin(100.0).to_bits(), @@10@@);
    assert_eq!(super::sin(1000.0).to_bits(), @@11@@);
    assert_eq!(super::sin(10000.0).to_bits(), @@12@@);
    assert_eq!(super::sin(1000000.0).to_bits(), @@13@@);
    assert_eq!(super::sin(-1000000.0).to_bits(), @@14@@);
    assert_eq!(super::sin(12345.6789).to_bits(), @@15@@);
    assert_eq!(super::sin(1.7976931348623157E308).to_bits(), @@16@@);
    assert_eq!(super::sin(2.2250738585072014E-308).to_bits(), @@17@@);
    assert_eq!(super::sin(4.9E-324).to_bits(), @@18@@);
    assert_eq!(super::sin(1.0E308).to_bits(), @@19@@);
    assert_eq!(super::cos(0.0).to_bits(), @@20@@);
    assert_eq!(super::cos(1.0).to_bits(), @@21@@);
    assert_eq!(super::cos(2.0).to_bits(), @@22@@);
    assert_eq!(super::cos(3.0).to_bits(), @@23@@);
    assert_eq!(super::cos(0.5).to_bits(), @@24@@);
    assert_eq!(super::cos(3.141592653589793).to_bits(), @@25@@);
    assert_eq!(super::cos(1.5707963267948966).to_bits(), @@26@@);
    assert_eq!(super::cos(6.283185307179586).to_bits(), @@27@@);
    assert_eq!(super::cos(-3.141592653589793).to_bits(), @@28@@);
    assert_eq!(super::cos(-1.5707963267948966).to_bits(), @@29@@);
    assert_eq!(super::cos(100.0).to_bits(), @@30@@);
    assert_eq!(super::cos(1000.0).to_bits(), @@31@@);
    assert_eq!(super::cos(10000.0).to_bits(), @@32@@);
    assert_eq!(super::cos(1000000.0).to_bits(), @@33@@);
    assert_eq!(super::cos(-1000000.0).to_bits(), @@34@@);
    assert_eq!(super::cos(12345.6789).to_bits(), @@35@@);
    assert_eq!(super::cos(1.7976931348623157E308).to_bits(), @@36@@);
    assert_eq!(super::cos(2.2250738585072014E-308).to_bits(), @@37@@);
    assert_eq!(super::cos(4.9E-324).to_bits(), @@38@@);
    assert_eq!(super::cos(1.0E308).to_bits(), @@39@@);
}

#[test]
fn golden_1() {
    // sqrt/floor/ceil/lfloor
    assert_eq!(super::sqrt(0.0f32).to_bits(), @@40@@);
    assert_eq!(super::sqrt(1.0f32).to_bits(), @@41@@);
    assert_eq!(super::sqrt(2.0f32).to_bits(), @@42@@);
    assert_eq!(super::sqrt(3.0f32).to_bits(), @@43@@);
    assert_eq!(super::sqrt(0.25f32).to_bits(), @@44@@);
    assert_eq!(super::sqrt(100.0f32).to_bits(), @@45@@);
    assert_eq!(super::sqrt(3.4028235E38f32).to_bits(), @@46@@);
    assert_eq!(super::sqrt(1.4E-45f32).to_bits(), @@47@@);
    assert_eq!(super::sqrt(f32::NAN).to_bits(), @@48@@);
    assert_eq!(super::sqrt(f32::INFINITY).to_bits(), @@49@@);
    assert_eq!(super::sqrt(4.0f32).to_bits(), @@50@@);
    assert_eq!(super::sqrt(1.0E30f32).to_bits(), @@51@@);
    assert_eq!(super::floor_d(-1.5), @@52@@);
    assert_eq!(super::floor_d(-1.0), @@53@@);
    assert_eq!(super::floor_d(-0.5), @@54@@);
    assert_eq!(super::floor_d(0.5), @@55@@);
    assert_eq!(super::floor_d(1.5), @@56@@);
    assert_eq!(super::floor_d(3.9), @@57@@);
    assert_eq!(super::floor_d(-3.9), @@58@@);
    assert_eq!(super::floor_d(f64::NAN), @@59@@);
    assert_eq!(super::floor_d(f64::INFINITY), @@60@@);
    assert_eq!(super::floor_d(1.7976931348623157E308), @@61@@);
    assert_eq!(super::floor_d(4.9E-324), @@62@@);
    assert_eq!(super::floor_d(2.147483647E9), @@63@@);
    assert_eq!(super::floor_d(2.147483648E9), @@64@@);
    assert_eq!(super::floor_d(-2.147483648E9), @@65@@);
    assert_eq!(super::floor_d(-2.147483649E9), @@66@@);
    assert_eq!(super::floor_d(1.0E30), @@67@@);
    assert_eq!(super::floor_d(-1.0E30), @@68@@);
    assert_eq!(super::floor_d(4.9E-324), @@69@@);
    assert_eq!(super::floor(-1.5f32), @@70@@);
    assert_eq!(super::floor(-1.0f32), @@71@@);
    assert_eq!(super::floor(-0.5f32), @@72@@);
    assert_eq!(super::floor(0.5f32), @@73@@);
    assert_eq!(super::floor(1.5f32), @@74@@);
    assert_eq!(super::floor(3.9f32), @@75@@);
    assert_eq!(super::floor(-3.9f32), @@76@@);
    assert_eq!(super::floor(f32::NAN), @@77@@);
    assert_eq!(super::floor(3.4028235E38f32), @@78@@);
    assert_eq!(super::floor(1.4E-45f32), @@79@@);
    assert_eq!(super::floor(2.1474836E9f32), @@80@@);
    assert_eq!(super::floor(2.1474836E9f32), @@81@@);
    assert_eq!(super::floor(-2.1474836E9f32), @@82@@);
    assert_eq!(super::floor(-2.1474836E9f32), @@83@@);
    assert_eq!(super::ceil(-1.5f32), @@84@@);
    assert_eq!(super::ceil(-1.0f32), @@85@@);
    assert_eq!(super::ceil(-0.5f32), @@86@@);
    assert_eq!(super::ceil(0.5f32), @@87@@);
    assert_eq!(super::ceil(1.5f32), @@88@@);
    assert_eq!(super::ceil(3.9f32), @@89@@);
    assert_eq!(super::ceil(-3.9f32), @@90@@);
    assert_eq!(super::ceil(f32::NAN), @@91@@);
    assert_eq!(super::ceil(3.4028235E38f32), @@92@@);
    assert_eq!(super::ceil(1.4E-45f32), @@93@@);
    assert_eq!(super::ceil(2.1474836E9f32), @@94@@);
    assert_eq!(super::ceil(2.1474836E9f32), @@95@@);
    assert_eq!(super::ceil(-2.1474836E9f32), @@96@@);
    assert_eq!(super::ceil(-2.1474836E9f32), @@97@@);
    assert_eq!(super::ceil_d(-1.5), @@98@@);
    assert_eq!(super::ceil_d(-1.0), @@99@@);
    assert_eq!(super::ceil_d(-0.5), @@100@@);
    assert_eq!(super::ceil_d(0.5), @@101@@);
    assert_eq!(super::ceil_d(1.5), @@102@@);
    assert_eq!(super::ceil_d(3.9), @@103@@);
    assert_eq!(super::ceil_d(-3.9), @@104@@);
    assert_eq!(super::ceil_d(f64::NAN), @@105@@);
    assert_eq!(super::ceil_d(f64::INFINITY), @@106@@);
    assert_eq!(super::ceil_d(1.7976931348623157E308), @@107@@);
    assert_eq!(super::ceil_d(4.9E-324), @@108@@);
    assert_eq!(super::ceil_d(2.147483647E9), @@109@@);
    assert_eq!(super::ceil_d(2.147483648E9), @@110@@);
    assert_eq!(super::ceil_d(-2.147483648E9), @@111@@);
    assert_eq!(super::ceil_d(-2.147483649E9), @@112@@);
    assert_eq!(super::ceil_d(1.0E30), @@113@@);
    assert_eq!(super::ceil_d(-1.0E30), @@114@@);
    assert_eq!(super::ceil_d(4.9E-324), @@115@@);
    assert_eq!(super::lfloor(-1.5), @@116@@);
    assert_eq!(super::lfloor(-1.0), @@117@@);
    assert_eq!(super::lfloor(-0.5), @@118@@);
    assert_eq!(super::lfloor(0.5), @@119@@);
    assert_eq!(super::lfloor(1.5), @@120@@);
    assert_eq!(super::lfloor(3.9), @@121@@);
    assert_eq!(super::lfloor(-3.9), @@122@@);
    assert_eq!(super::lfloor(f64::NAN), @@123@@);
    assert_eq!(super::lfloor(f64::INFINITY), @@124@@);
    assert_eq!(
        super::lfloor(1.7976931348623157E308),
@@125@@);
    assert_eq!(super::lfloor(4.9E-324), @@126@@);
    assert_eq!(super::lfloor(2.147483647E9), @@127@@);
    assert_eq!(super::lfloor(2.147483648E9), @@128@@);
    assert_eq!(super::lfloor(-2.147483648E9), @@129@@);
    assert_eq!(super::lfloor(-2.147483649E9), @@130@@);
    assert_eq!(super::lfloor(1.0E30), @@131@@);
    assert_eq!(super::lfloor(-1.0E30), @@132@@);
    assert_eq!(super::lfloor(4.9E-324), @@133@@);
    assert_eq!(super::ceil_long(-1.5), @@134@@);
    assert_eq!(super::ceil_long(-1.0), @@135@@);
    assert_eq!(super::ceil_long(-0.5), @@136@@);
    assert_eq!(super::ceil_long(0.5), @@137@@);
    assert_eq!(super::ceil_long(1.5), @@138@@);
    assert_eq!(super::ceil_long(3.9), @@139@@);
    assert_eq!(super::ceil_long(-3.9), @@140@@);
    assert_eq!(super::ceil_long(f64::NAN), @@141@@);
    assert_eq!(super::ceil_long(f64::INFINITY), @@142@@);
    assert_eq!(
        super::ceil_long(1.7976931348623157E308),
@@143@@);
    assert_eq!(super::ceil_long(4.9E-324), @@144@@);
    assert_eq!(super::ceil_long(2.147483647E9), @@145@@);
    assert_eq!(super::ceil_long(2.147483648E9), @@146@@);
    assert_eq!(super::ceil_long(-2.147483648E9), @@147@@);
    assert_eq!(super::ceil_long(-2.147483649E9), @@148@@);
    assert_eq!(super::ceil_long(1.0E30), @@149@@);
    assert_eq!(super::ceil_long(-1.0E30), @@150@@);
    assert_eq!(super::ceil_long(4.9E-324), @@151@@);
}

#[test]
fn golden_2() {
    // abs/absMax
    assert_eq!(super::abs_i32(0i32), @@152@@);
    assert_eq!(super::abs_i32(1i32), @@153@@);
    assert_eq!(super::abs_i32(-1i32), @@154@@);
    assert_eq!(super::abs_i32(42i32), @@155@@);
    assert_eq!(super::abs_i32(-42i32), @@156@@);
    assert_eq!(super::abs_i32(2147483647i32), @@157@@);
    assert_eq!(super::abs_i32(-2147483648i32), @@158@@);
    assert_eq!(super::abs_max(0i32, 1i32), @@159@@);
    assert_eq!(super::abs_max(1i32, 2i32), @@160@@);
    assert_eq!(super::abs_max(-1i32, 0i32), @@161@@);
    assert_eq!(super::abs_max(42i32, 43i32), @@162@@);
    assert_eq!(super::abs_max(-42i32, -41i32), @@163@@);
    assert_eq!(super::abs_max(2147483647i32, -2147483648i32), @@164@@);
    assert_eq!(
        super::abs_max(-2147483648i32, -2147483647i32),
@@165@@);
    assert_eq!(super::abs_max(0i32, -1i32), @@166@@);
    assert_eq!(super::abs_max(1i32, 0i32), @@167@@);
    assert_eq!(super::abs_max(-1i32, -2i32), @@168@@);
    assert_eq!(super::abs_max(42i32, 41i32), @@169@@);
    assert_eq!(super::abs_max(-42i32, -43i32), @@170@@);
    assert_eq!(super::abs_max(2147483647i32, 2147483646i32), @@171@@);
    assert_eq!(super::abs_max(-2147483648i32, 2147483647i32), @@172@@);
    assert_eq!(super::chessboard_distance(3, 7, -2, 10), @@173@@);
    assert_eq!(super::chessboard_distance(-5, 0, 5, 0), @@174@@);
    assert_eq!(super::abs(-1.5f32).to_bits(), @@175@@);
    assert_eq!(super::abs(1.5f32).to_bits(), @@176@@);
    assert_eq!(super::abs(f32::NAN).to_bits(), @@177@@);
    assert_eq!(super::abs(1.4E-45f32).to_bits(), @@178@@);
}

#[test]
fn golden_3() {
    // clamp
    assert_eq!(super::clamp(5, 0, 10), @@179@@);
    assert_eq!(super::clamp(-5, 0, 10), @@180@@);
    assert_eq!(super::clamp(15, 0, 10), @@181@@);
    assert_eq!(super::clamp(-2147483648, -1, 1), @@182@@);
    assert_eq!(super::clamp(2147483647, -1, 1), @@183@@);
    assert_eq!(super::clamp(-9223372036854775808i64, -1i64, 1i64), @@184@@);
    assert_eq!(super::clamp_f32(5.0f32, 0.0, 10.0).to_bits(), @@185@@);
    assert_eq!(super::clamp_f32(-5.0f32, 0.0, 10.0).to_bits(), @@186@@);
    assert_eq!(super::clamp_f32(15.0f32, 0.0, 10.0).to_bits(), @@187@@);
    assert_eq!(super::clamp_f32(f32::NAN, 0.0, 10.0).to_bits(), @@188@@);
    assert_eq!(
        super::clamp_f32(5.0f32, 0.0, f32::NAN).to_bits(),
@@189@@);
    assert_eq!(super::clamp_f32(0.5f32, 0.0, 10.0).to_bits(), @@190@@);
    assert_eq!(
        super::clamp_f64(5.0, 0.0, 10.0).to_bits(),
@@191@@);
    assert_eq!(
        super::clamp_f64(-5.0, 0.0, 10.0).to_bits(),
@@192@@);
    assert_eq!(
        super::clamp_f64(15.0, 0.0, 10.0).to_bits(),
@@193@@);
    assert_eq!(
        super::clamp_f64(f64::NAN, 0.0, 10.0).to_bits(),
@@194@@);
    assert_eq!(
        super::clamp_f64(5.0, 0.0, f64::NAN).to_bits(),
@@195@@);
    assert_eq!(
        super::clamp_f64(0.5, 0.0, 10.0).to_bits(),
@@196@@);
}

#[test]
fn golden_4() {
    // clampedLerp/lerp/lerpInt/lerpDiscrete
    assert_eq!(
        super::clamped_lerp(-1.0, 1.0, 5.0).to_bits(),
@@197@@);
    assert_eq!(
        super::clamped_lerp(0.0, 1.0, 5.0).to_bits(),
@@198@@);
    assert_eq!(
        super::clamped_lerp(0.25, 1.0, 5.0).to_bits(),
@@199@@);
    assert_eq!(
        super::clamped_lerp(0.5, 1.0, 5.0).to_bits(),
@@200@@);
    assert_eq!(
        super::clamped_lerp(0.75, 1.0, 5.0).to_bits(),
@@201@@);
    assert_eq!(
        super::clamped_lerp(1.0, 1.0, 5.0).to_bits(),
@@202@@);
    assert_eq!(
        super::clamped_lerp(2.0, 1.0, 5.0).to_bits(),
@@203@@);
    assert_eq!(
        super::clamped_lerp(f64::NAN, 1.0, 5.0).to_bits(),
@@204@@);
    assert_eq!(
        super::clamped_lerp_f32(-1.0f32, 1.0, 5.0).to_bits(),
@@205@@);
    assert_eq!(
        super::clamped_lerp_f32(0.0f32, 1.0, 5.0).to_bits(),
@@206@@);
    assert_eq!(
        super::clamped_lerp_f32(0.25f32, 1.0, 5.0).to_bits(),
@@207@@);
    assert_eq!(
        super::clamped_lerp_f32(0.5f32, 1.0, 5.0).to_bits(),
@@208@@);
    assert_eq!(
        super::clamped_lerp_f32(1.0f32, 1.0, 5.0).to_bits(),
@@209@@);
    assert_eq!(
        super::clamped_lerp_f32(2.0f32, 1.0, 5.0).to_bits(),
@@210@@);
    assert_eq!(
        super::clamped_lerp_f32(f32::NAN, 1.0, 5.0).to_bits(),
@@211@@);
    assert_eq!(super::lerp(-1.0, 1.0, 5.0).to_bits(), @@212@@);
    assert_eq!(super::lerp(0.0, 1.0, 5.0).to_bits(), @@213@@);
    assert_eq!(super::lerp(0.5, 1.0, 5.0).to_bits(), @@214@@);
    assert_eq!(super::lerp(1.0, 1.0, 5.0).to_bits(), @@215@@);
    assert_eq!(super::lerp(2.0, 1.0, 5.0).to_bits(), @@216@@);
    assert_eq!(
        super::lerp(f64::NAN, 1.0, 5.0).to_bits(),
@@217@@);
    assert_eq!(super::lerp_f32(-1.0f32, 1.0, 5.0).to_bits(), @@218@@);
    assert_eq!(super::lerp_f32(0.0f32, 1.0, 5.0).to_bits(), @@219@@);
    assert_eq!(super::lerp_f32(0.5f32, 1.0, 5.0).to_bits(), @@220@@);
    assert_eq!(super::lerp_f32(1.0f32, 1.0, 5.0).to_bits(), @@221@@);
    assert_eq!(super::lerp_f32(f32::NAN, 1.0, 5.0).to_bits(), @@222@@);
    assert_eq!(super::lerp_int(0.5, 10, 20), @@223@@);
    assert_eq!(super::lerp_int(-0.5, 10, 20), @@224@@);
    assert_eq!(super::lerp_int(0.0, 10, 20), @@225@@);
    assert_eq!(super::lerp_int(1.0, 10, 20), @@226@@);
    assert_eq!(super::lerp_discrete(0.5, 10, 20), @@227@@);
    assert_eq!(super::lerp_discrete(0.0, 10, 20), @@228@@);
    assert_eq!(super::lerp_discrete(0.5, 10, 11), @@229@@);
    assert_eq!(
        super::lerp2(0.5, 0.5, 0.0, 10.0, 20.0, 30.0).to_bits(),
@@230@@);
    assert_eq!(
        super::lerp3(0.5, 0.5, 0.5, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0).to_bits(),
@@231@@);
}

#[test]
fn golden_5() {
    // wrapDegrees(int)
    assert_eq!(super::wrap_degrees(180i32), @@232@@);
    assert_eq!(super::wrap_degrees(-180i32), @@233@@);
    assert_eq!(super::wrap_degrees(181i32), @@234@@);
    assert_eq!(super::wrap_degrees(-181i32), @@235@@);
    assert_eq!(super::wrap_degrees(360i32), @@236@@);
    assert_eq!(super::wrap_degrees(-360i32), @@237@@);
    assert_eq!(super::wrap_degrees(540i32), @@238@@);
    assert_eq!(super::wrap_degrees(-540i32), @@239@@);
    assert_eq!(super::wrap_degrees(720i32), @@240@@);
    assert_eq!(super::wrap_degrees(0i32), @@241@@);
    assert_eq!(super::wrap_degrees(359i32), @@242@@);
    assert_eq!(super::wrap_degrees(-359i32), @@243@@);
    assert_eq!(super::wrap_degrees(179i32), @@244@@);
    assert_eq!(super::wrap_degrees(-179i32), @@245@@);
    assert_eq!(super::wrap_degrees(2147483647i32), @@246@@);
    assert_eq!(super::wrap_degrees(-2147483648i32), @@247@@);
    assert_eq!(super::wrap_degrees(1000000i32), @@248@@);
    assert_eq!(super::wrap_degrees(-1000000i32), @@249@@);
}

#[test]
fn golden_6() {
    // wrapDegrees(float)
    assert_eq!(super::wrap_degrees_f32(180.0f32).to_bits(), @@250@@);
    assert_eq!(super::wrap_degrees_f32(-180.0f32).to_bits(), @@251@@);
    assert_eq!(super::wrap_degrees_f32(181.0f32).to_bits(), @@252@@);
    assert_eq!(super::wrap_degrees_f32(-181.0f32).to_bits(), @@253@@);
    assert_eq!(super::wrap_degrees_f32(360.0f32).to_bits(), @@254@@);
    assert_eq!(super::wrap_degrees_f32(-360.0f32).to_bits(), @@255@@);
    assert_eq!(super::wrap_degrees_f32(540.0f32).to_bits(), @@256@@);
    assert_eq!(super::wrap_degrees_f32(-540.0f32).to_bits(), @@257@@);
    assert_eq!(super::wrap_degrees_f32(720.0f32).to_bits(), @@258@@);
    assert_eq!(super::wrap_degrees_f32(0.0f32).to_bits(), @@259@@);
    assert_eq!(super::wrap_degrees_f32(359.9f32).to_bits(), @@260@@);
    assert_eq!(super::wrap_degrees_f32(-359.9f32).to_bits(), @@261@@);
    assert_eq!(super::wrap_degrees_f32(179.999f32).to_bits(), @@262@@);
    assert_eq!(super::wrap_degrees_f32(f32::NAN).to_bits(), @@263@@);
    assert_eq!(
        super::wrap_degrees_f32(3.4028235E38f32).to_bits(),
@@264@@);
    assert_eq!(
        super::wrap_degrees_f32(-3.4028235E38f32).to_bits(),
@@265@@);
    assert_eq!(super::wrap_degrees_f32(1.4E-45f32).to_bits(), @@266@@);
    assert_eq!(super::wrap_degrees_f32(-1.4E-45f32).to_bits(), @@267@@);
    assert_eq!(super::wrap_degrees_f32(-0.0f32).to_bits(), @@268@@);
    assert_eq!(super::wrap_degrees_f32(1.0E-4f32).to_bits(), @@269@@);
    assert_eq!(super::wrap_degrees_f32(270.0f32).to_bits(), @@270@@);
    assert_eq!(super::wrap_degrees_f32(-270.0f32).to_bits(), @@271@@);
    assert_eq!(super::wrap_degrees_f32(90.0f32).to_bits(), @@272@@);
    assert_eq!(super::wrap_degrees_f32(-90.0f32).to_bits(), @@273@@);
    assert_eq!(super::wrap_degrees_i64(180i64).to_bits(), @@274@@);
    assert_eq!(super::wrap_degrees_i64(-180i64).to_bits(), @@275@@);
    assert_eq!(super::wrap_degrees_i64(181i64).to_bits(), @@276@@);
    assert_eq!(super::wrap_degrees_i64(-181i64).to_bits(), @@277@@);
    assert_eq!(super::wrap_degrees_i64(360i64).to_bits(), @@278@@);
    assert_eq!(super::wrap_degrees_i64(-360i64).to_bits(), @@279@@);
    assert_eq!(super::wrap_degrees_i64(540i64).to_bits(), @@280@@);
    assert_eq!(super::wrap_degrees_i64(-540i64).to_bits(), @@281@@);
    assert_eq!(
        super::wrap_degrees_i64(9223372036854775807i64).to_bits(),
@@282@@);
    assert_eq!(
        super::wrap_degrees_i64(-9223372036854775808i64).to_bits(),
@@283@@);
    assert_eq!(super::wrap_degrees_i64(0i64).to_bits(), @@284@@);
    assert_eq!(super::wrap_degrees_i64(359i64).to_bits(), @@285@@);
    assert_eq!(super::wrap_degrees_i64(1i64).to_bits(), @@286@@);
    assert_eq!(super::wrap_degrees_f64(180.0).to_bits(), @@287@@);
    assert_eq!(
        super::wrap_degrees_f64(-180.0).to_bits(),
@@288@@);
    assert_eq!(super::wrap_degrees_f64(181.0).to_bits(), @@289@@);
    assert_eq!(
        super::wrap_degrees_f64(-181.0).to_bits(),
@@290@@);
    assert_eq!(super::wrap_degrees_f64(360.0).to_bits(), @@291@@);
    assert_eq!(
        super::wrap_degrees_f64(-360.0).to_bits(),
@@292@@);
    assert_eq!(super::wrap_degrees_f64(540.0).to_bits(), @@293@@);
    assert_eq!(
        super::wrap_degrees_f64(-540.0).to_bits(),
@@294@@);
    assert_eq!(
        super::wrap_degrees_f64(179.999).to_bits(),
@@295@@);
    assert_eq!(
        super::wrap_degrees_f64(1.0E18).to_bits(),
@@296@@);
    assert_eq!(
        super::wrap_degrees_f64(-1.0E18).to_bits(),
@@297@@);
    assert_eq!(
        super::wrap_degrees_f64(f64::NAN).to_bits(),
@@298@@);
    assert_eq!(
        super::wrap_degrees_f64(f64::INFINITY).to_bits(),
@@299@@);
    assert_eq!(super::wrap_degrees_f64(0.0).to_bits(), @@300@@);
    assert_eq!(super::wrap_degrees_f64(-0.0).to_bits(), @@301@@);
    assert_eq!(super::wrap_degrees_f64(90.0).to_bits(), @@302@@);
    assert_eq!(super::wrap_degrees_f64(270.0).to_bits(), @@303@@);
    assert_eq!(super::wrap_degrees90(45.0f32).to_bits(), @@304@@);
    assert_eq!(super::wrap_degrees90(-45.0f32).to_bits(), @@305@@);
    assert_eq!(super::wrap_degrees90(90.0f32).to_bits(), @@306@@);
    assert_eq!(super::wrap_degrees90(-90.0f32).to_bits(), @@307@@);
    assert_eq!(super::wrap_degrees90(44.0f32).to_bits(), @@308@@);
    assert_eq!(super::wrap_degrees90(-44.0f32).to_bits(), @@309@@);
    assert_eq!(super::wrap_degrees90(46.0f32).to_bits(), @@310@@);
    assert_eq!(super::wrap_degrees90(-46.0f32).to_bits(), @@311@@);
    assert_eq!(super::wrap_degrees90(135.0f32).to_bits(), @@312@@);
    assert_eq!(super::wrap_degrees90(-135.0f32).to_bits(), @@313@@);
    assert_eq!(super::wrap_degrees90(180.0f32).to_bits(), @@314@@);
    assert_eq!(super::wrap_degrees90(-180.0f32).to_bits(), @@315@@);
    assert_eq!(super::wrap_degrees90(0.0f32).to_bits(), @@316@@);
    assert_eq!(super::wrap_degrees90(f32::NAN).to_bits(), @@317@@);
    assert_eq!(super::wrap_degrees90(89.0f32).to_bits(), @@318@@);
    assert_eq!(super::wrap_degrees90(179.0f32).to_bits(), @@319@@);
}

#[test]
fn golden_7() {
    // degreesDifference/rotateIfNecessary/approach
    assert_eq!(super::degrees_difference(10.0, 350.0).to_bits(), @@320@@);
    assert_eq!(super::degrees_difference(350.0, 10.0).to_bits(), @@321@@);
    assert_eq!(super::degrees_difference(0.0, 180.0).to_bits(), @@322@@);
    assert_eq!(
        super::degrees_difference_abs(10.0, 350.0).to_bits(),
@@323@@);
    assert_eq!(
        super::rotate_if_necessary(10.0, 350.0, 30.0).to_bits(),
@@324@@);
    assert_eq!(
        super::rotate_if_necessary(0.0, 20.0, 90.0).to_bits(),
@@325@@);
    assert_eq!(super::approach(0.0, 10.0, 3.0).to_bits(), @@326@@);
    assert_eq!(super::approach(10.0, 0.0, 3.0).to_bits(), @@327@@);
    assert_eq!(super::approach(5.0, 5.0, 3.0).to_bits(), @@328@@);
    assert_eq!(super::approach(0.0, 10.0, -3.0).to_bits(), @@329@@);
    assert_eq!(
        super::approach_degrees(10.0, 350.0, 45.0).to_bits(),
@@330@@);
}

#[test]
fn golden_8() {
    // getInt / smallestSquareSide / powerOfTwo
    assert_eq!(super::get_int(Some("42"), -1), @@331@@);
    assert_eq!(super::get_int(Some("-42"), -1), @@332@@);
    assert_eq!(super::get_int(Some("+42"), -1), @@333@@);
    assert_eq!(super::get_int(Some(""), -1), @@334@@);
    assert_eq!(super::get_int(Some(" "), -1), @@335@@);
    assert_eq!(super::get_int(Some(" 42"), -1), @@336@@);
    assert_eq!(super::get_int(Some("42 "), -1), @@337@@);
    assert_eq!(super::get_int(Some("42abc"), -1), @@338@@);
    assert_eq!(super::get_int(Some("abc"), -1), @@339@@);
    assert_eq!(super::get_int(Some("2147483647"), -1), @@340@@);
    assert_eq!(super::get_int(Some("2147483648"), -1), @@341@@);
    assert_eq!(super::get_int(Some("-2147483648"), -1), @@342@@);
    assert_eq!(super::get_int(Some("-2147483649"), -1), @@343@@);
    assert_eq!(super::get_int(Some("9223372036854775808"), -1), @@344@@);
    assert_eq!(super::get_int(Some("9223372036854775807"), -1), @@345@@);
    assert_eq!(super::get_int(Some("18446744073709551616"), -1), @@346@@);
    assert_eq!(super::get_int(Some("0x10"), -1), @@347@@);
    assert_eq!(super::get_int(Some("1e3"), -1), @@348@@);
    assert_eq!(super::get_int(None, -1), @@349@@);
    assert_eq!(super::smallest_square_side(0), @@350@@);
    assert_eq!(super::smallest_square_side(1), @@351@@);
    assert_eq!(super::smallest_square_side(2), @@352@@);
    assert_eq!(super::smallest_square_side(3), @@353@@);
    assert_eq!(super::smallest_square_side(4), @@354@@);
    assert_eq!(super::smallest_square_side(5), @@355@@);
    assert_eq!(super::smallest_square_side(8), @@356@@);
    assert_eq!(super::smallest_square_side(9), @@357@@);
    assert_eq!(super::smallest_square_side(10), @@358@@);
    assert_eq!(super::smallest_square_side(15), @@359@@);
    assert_eq!(super::smallest_square_side(16), @@360@@);
    assert_eq!(super::smallest_square_side(17), @@361@@);
    assert_eq!(super::smallest_square_side(1023), @@362@@);
    assert_eq!(super::smallest_square_side(1024), @@363@@);
    assert_eq!(super::smallest_square_side(1025), @@364@@);
    assert_eq!(super::smallest_square_side(2147483647), @@365@@);
    assert_eq!(super::smallest_encompassing_power_of_two(0), @@366@@);
    assert_eq!(super::smallest_encompassing_power_of_two(1), @@367@@);
    assert_eq!(super::smallest_encompassing_power_of_two(2), @@368@@);
    assert_eq!(super::smallest_encompassing_power_of_two(3), @@369@@);
    assert_eq!(super::smallest_encompassing_power_of_two(4), @@370@@);
    assert_eq!(super::smallest_encompassing_power_of_two(5), @@371@@);
    assert_eq!(super::smallest_encompassing_power_of_two(8), @@372@@);
    assert_eq!(super::smallest_encompassing_power_of_two(9), @@373@@);
    assert_eq!(super::smallest_encompassing_power_of_two(16), @@374@@);
    assert_eq!(super::smallest_encompassing_power_of_two(31), @@375@@);
    assert_eq!(super::smallest_encompassing_power_of_two(32), @@376@@);
    assert_eq!(super::smallest_encompassing_power_of_two(1023), @@377@@);
    assert_eq!(super::smallest_encompassing_power_of_two(1024), @@378@@);
    assert_eq!(
        super::smallest_encompassing_power_of_two(2147483647),
@@379@@);
    assert_eq!(
        super::smallest_encompassing_power_of_two(-2147483648),
@@380@@);
    assert_eq!(super::smallest_encompassing_power_of_two(-1), @@381@@);
    assert_eq!(super::smallest_encompassing_power_of_two(-2), @@382@@);
    assert_eq!(super::is_power_of_two(0i32), @@383@@);
    assert_eq!(super::is_power_of_two(1i32), @@384@@);
    assert_eq!(super::is_power_of_two(2i32), @@385@@);
    assert_eq!(super::is_power_of_two(3i32), @@386@@);
    assert_eq!(super::is_power_of_two(4i32), @@387@@);
    assert_eq!(super::is_power_of_two(7i32), @@388@@);
    assert_eq!(super::is_power_of_two(8i32), @@389@@);
    assert_eq!(super::is_power_of_two(9i32), @@390@@);
    assert_eq!(super::is_power_of_two(1024i32), @@391@@);
    assert_eq!(super::is_power_of_two(-2147483648i32), @@392@@);
    assert_eq!(super::is_power_of_two(-1i32), @@393@@);
    assert_eq!(super::is_power_of_two(-2i32), @@394@@);
    assert_eq!(super::is_power_of_two_i64(0i64), @@395@@);
    assert_eq!(super::is_power_of_two_i64(1i64), @@396@@);
    assert_eq!(super::is_power_of_two_i64(2i64), @@397@@);
    assert_eq!(super::is_power_of_two_i64(3i64), @@398@@);
    assert_eq!(super::is_power_of_two_i64(4i64), @@399@@);
    assert_eq!(super::is_power_of_two_i64(7i64), @@400@@);
    assert_eq!(super::is_power_of_two_i64(8i64), @@401@@);
    assert_eq!(super::is_power_of_two_i64(9i64), @@402@@);
    assert_eq!(super::is_power_of_two_i64(1024i64), @@403@@);
    assert_eq!(super::is_power_of_two_i64(-9223372036854775808i64), @@404@@);
    assert_eq!(super::is_power_of_two_i64(-1i64), @@405@@);
    assert_eq!(super::is_power_of_two_i64(4611686018427387904i64), @@406@@);
    assert_eq!(super::is_power_of_two_i64(-9223372036854775808i64), @@407@@);
    assert_eq!(super::ceillog2(0), @@408@@);
    assert_eq!(super::ceillog2(1), @@409@@);
    assert_eq!(super::ceillog2(2), @@410@@);
    assert_eq!(super::ceillog2(3), @@411@@);
    assert_eq!(super::ceillog2(4), @@412@@);
    assert_eq!(super::ceillog2(5), @@413@@);
    assert_eq!(super::ceillog2(8), @@414@@);
    assert_eq!(super::ceillog2(9), @@415@@);
    assert_eq!(super::ceillog2(16), @@416@@);
    assert_eq!(super::ceillog2(17), @@417@@);
    assert_eq!(super::ceillog2(1023), @@418@@);
    assert_eq!(super::ceillog2(1024), @@419@@);
    assert_eq!(super::ceillog2(1025), @@420@@);
    assert_eq!(super::ceillog2(2147483647), @@421@@);
    assert_eq!(super::ceillog2(-2147483648), @@422@@);
    assert_eq!(super::ceillog2(7), @@423@@);
    assert_eq!(super::ceillog2(6), @@424@@);
    assert_eq!(super::log2(0), @@425@@);
    assert_eq!(super::log2(1), @@426@@);
    assert_eq!(super::log2(2), @@427@@);
    assert_eq!(super::log2(3), @@428@@);
    assert_eq!(super::log2(4), @@429@@);
    assert_eq!(super::log2(5), @@430@@);
    assert_eq!(super::log2(8), @@431@@);
    assert_eq!(super::log2(9), @@432@@);
    assert_eq!(super::log2(16), @@433@@);
    assert_eq!(super::log2(17), @@434@@);
    assert_eq!(super::log2(1023), @@435@@);
    assert_eq!(super::log2(1024), @@436@@);
    assert_eq!(super::log2(1025), @@437@@);
    assert_eq!(super::log2(2147483647), @@438@@);
    assert_eq!(super::log2(-2147483648), @@439@@);
    assert_eq!(super::log2(7), @@440@@);
    assert_eq!(super::log2(6), @@441@@);
}

#[test]
fn golden_9() {
    // frac/getSeed/murmur
    assert_eq!(super::frac(1.5f32).to_bits(), @@442@@);
    assert_eq!(super::frac(-1.5f32).to_bits(), @@443@@);
    assert_eq!(super::frac(0.5f32).to_bits(), @@444@@);
    assert_eq!(super::frac(-0.5f32).to_bits(), @@445@@);
    assert_eq!(super::frac(3.9f32).to_bits(), @@446@@);
    assert_eq!(super::frac(-3.9f32).to_bits(), @@447@@);
    assert_eq!(super::frac(0.0f32).to_bits(), @@448@@);
    assert_eq!(super::frac(-0.0f32).to_bits(), @@449@@);
    assert_eq!(super::frac(f32::NAN).to_bits(), @@450@@);
    assert_eq!(super::frac_f64(1.5).to_bits(), @@451@@);
    assert_eq!(super::frac_f64(-1.5).to_bits(), @@452@@);
    assert_eq!(super::frac_f64(0.5).to_bits(), @@453@@);
    assert_eq!(super::frac_f64(-0.5).to_bits(), @@454@@);
    assert_eq!(super::frac_f64(3.9).to_bits(), @@455@@);
    assert_eq!(super::frac_f64(-3.9).to_bits(), @@456@@);
    assert_eq!(super::frac_f64(0.0).to_bits(), @@457@@);
    assert_eq!(super::frac_f64(-0.0).to_bits(), @@458@@);
    assert_eq!(super::frac_f64(f64::NAN).to_bits(), @@459@@);
    assert_eq!(super::frac_f64(1.0E20).to_bits(), @@460@@);
    assert_eq!(super::frac_f64(-1.0E20).to_bits(), @@461@@);
    assert_eq!(super::get_seed(1, 2, 3), @@462@@);
    assert_eq!(super::get_seed(0, 0, 0), @@463@@);
    assert_eq!(super::get_seed(-1, 1, -1), @@464@@);
    assert_eq!(
        super::get_seed(2147483647, -2147483648, 0),
@@465@@);
    assert_eq!(super::get_seed(7, 7, 7), @@466@@);
    assert_eq!(super::get_seed(12345, -9876, 55555), @@467@@);
    assert_eq!(super::murmur_hash3_mixer(0), @@468@@);
    assert_eq!(super::murmur_hash3_mixer(1), @@469@@);
    assert_eq!(super::murmur_hash3_mixer(-1), @@470@@);
    assert_eq!(super::murmur_hash3_mixer(2147483647), @@471@@);
    assert_eq!(super::murmur_hash3_mixer(-2147483648), @@472@@);
    assert_eq!(super::murmur_hash3_mixer(12345), @@473@@);
    assert_eq!(super::murmur_hash3_mixer(-12345), @@474@@);
    assert_eq!(super::murmur_hash3_mixer(987654321), @@475@@);
}

#[test]
fn golden_10() {
    // positiveModulo/isMultipleOf/floorDiv
    assert_eq!(super::positive_modulo(7, 3), @@476@@);
    assert_eq!(super::positive_modulo(-7, 3), @@477@@);
    assert_eq!(super::positive_modulo(7, -3), @@478@@);
    assert_eq!(super::positive_modulo(-7, -3), @@479@@);
    assert_eq!(super::positive_modulo(-2147483648, 3), @@480@@);
    assert_eq!(super::positive_modulo(2147483647, 2), @@481@@);
    assert_eq!(super::positive_modulo(0, 5), @@482@@);
    assert_eq!(super::positive_modulo(-1, 5), @@483@@);
    assert_eq!(super::positive_modulo(-6, 5), @@484@@);
    assert_eq!(super::positive_modulo(6, 5), @@485@@);
    assert_eq!(super::floor_div(7, 3), @@486@@);
    assert_eq!(super::floor_div(-7, 3), @@487@@);
    assert_eq!(super::floor_div(7, -3), @@488@@);
    assert_eq!(super::floor_div(-7, -3), @@489@@);
    assert_eq!(super::floor_div(-2147483648, 3), @@490@@);
    assert_eq!(super::floor_div(2147483647, 2), @@491@@);
    assert_eq!(super::floor_div(0, 5), @@492@@);
    assert_eq!(super::floor_div(-1, 5), @@493@@);
    assert_eq!(super::floor_div(-6, 5), @@494@@);
    assert_eq!(super::floor_div(6, 5), @@495@@);
    assert_eq!(super::is_multiple_of(7, 3), @@496@@);
    assert_eq!(super::is_multiple_of(-7, 3), @@497@@);
    assert_eq!(super::is_multiple_of(7, -3), @@498@@);
    assert_eq!(super::is_multiple_of(-7, -3), @@499@@);
    assert_eq!(super::is_multiple_of(-2147483648, 3), @@500@@);
    assert_eq!(super::is_multiple_of(2147483647, 2), @@501@@);
    assert_eq!(super::is_multiple_of(0, 5), @@502@@);
    assert_eq!(super::is_multiple_of(-1, 5), @@503@@);
    assert_eq!(super::is_multiple_of(-6, 5), @@504@@);
    assert_eq!(super::is_multiple_of(6, 5), @@505@@);
    assert_eq!(
        super::positive_modulo_f32(7.5f32, 3.0f32).to_bits(),
@@506@@);
    assert_eq!(
        super::positive_modulo_f32(-7.5f32, 3.0f32).to_bits(),
@@507@@);
    assert_eq!(
        super::positive_modulo_f32(7.5f32, -3.0f32).to_bits(),
@@508@@);
    assert_eq!(
        super::positive_modulo_f32(-7.5f32, -3.0f32).to_bits(),
@@509@@);
    assert_eq!(
        super::positive_modulo_f32(0.0f32, 3.0f32).to_bits(),
@@510@@);
    assert_eq!(
        super::positive_modulo_f32(-0.0f32, 3.0f32).to_bits(),
@@511@@);
    assert_eq!(
        super::positive_modulo_f64(7.5, 3.0).to_bits(),
@@512@@);
    assert_eq!(
        super::positive_modulo_f64(-7.5, 3.0).to_bits(),
@@513@@);
    assert_eq!(
        super::positive_modulo_f64(7.5, -3.0).to_bits(),
@@514@@);
    assert_eq!(
        super::positive_modulo_f64(-7.5, -3.0).to_bits(),
@@515@@);
    assert_eq!(
        super::positive_modulo_f64(0.0, 3.0).to_bits(),
@@516@@);
    assert_eq!(
        super::positive_modulo_f64(-0.0, 3.0).to_bits(),
@@517@@);
}

#[test]
fn golden_11() {
    // packDegrees/unpackDegrees
    assert_eq!(super::pack_degrees(0.0f32), @@518@@);
    assert_eq!(super::pack_degrees(45.0f32), @@519@@);
    assert_eq!(super::pack_degrees(90.0f32), @@520@@);
    assert_eq!(super::pack_degrees(180.0f32), @@521@@);
    assert_eq!(super::pack_degrees(270.0f32), @@522@@);
    assert_eq!(super::pack_degrees(359.0f32), @@523@@);
    assert_eq!(super::pack_degrees(-45.0f32), @@524@@);
    assert_eq!(super::pack_degrees(-90.0f32), @@525@@);
    assert_eq!(super::pack_degrees(-180.0f32), @@526@@);
    assert_eq!(super::pack_degrees(-270.0f32), @@527@@);
    assert_eq!(super::pack_degrees(360.0f32), @@528@@);
    assert_eq!(super::pack_degrees(720.0f32), @@529@@);
    assert_eq!(super::pack_degrees(f32::NAN), @@530@@);
    assert_eq!(super::pack_degrees(1.0f32), @@531@@);
    assert_eq!(super::pack_degrees(-1.0f32), @@532@@);
    assert_eq!(super::unpack_degrees(0).to_bits(), @@533@@);
    assert_eq!(super::unpack_degrees(45).to_bits(), @@534@@);
    assert_eq!(super::unpack_degrees(90).to_bits(), @@535@@);
    assert_eq!(super::unpack_degrees(-1).to_bits(), @@536@@);
    assert_eq!(super::unpack_degrees(-90).to_bits(), @@537@@);
    assert_eq!(super::unpack_degrees(-128).to_bits(), @@538@@);
    assert_eq!(super::unpack_degrees(127).to_bits(), @@539@@);
    assert_eq!(super::unpack_degrees(1).to_bits(), @@540@@);
    assert_eq!(super::unpack_degrees(-2).to_bits(), @@541@@);
}

#[test]
fn golden_12() {
    // atan2
    assert_eq!(super::atan2(0.0, 0.0).to_bits(), @@542@@);
    assert_eq!(super::atan2(0.0, 1.0).to_bits(), @@543@@);
    assert_eq!(super::atan2(1.0, 0.0).to_bits(), @@544@@);
    assert_eq!(super::atan2(-1.0, 0.0).to_bits(), @@545@@);
    assert_eq!(super::atan2(0.0, -1.0).to_bits(), @@546@@);
    assert_eq!(super::atan2(1.0, 1.0).to_bits(), @@547@@);
    assert_eq!(super::atan2(-1.0, -1.0).to_bits(), @@548@@);
    assert_eq!(super::atan2(1.0, -1.0).to_bits(), @@549@@);
    assert_eq!(super::atan2(-1.0, 1.0).to_bits(), @@550@@);
    assert_eq!(super::atan2(3.0, 4.0).to_bits(), @@551@@);
    assert_eq!(super::atan2(4.0, 3.0).to_bits(), @@552@@);
    assert_eq!(super::atan2(0.1, 0.2).to_bits(), @@553@@);
    assert_eq!(super::atan2(-0.1, 0.2).to_bits(), @@554@@);
    assert_eq!(super::atan2(0.1, -0.2).to_bits(), @@555@@);
    assert_eq!(super::atan2(-0.1, -0.2).to_bits(), @@556@@);
    assert_eq!(super::atan2(f64::NAN, 1.0).to_bits(), @@557@@);
    assert_eq!(super::atan2(1.0, f64::NAN).to_bits(), @@558@@);
    assert_eq!(
        super::atan2(f64::INFINITY, 1.0).to_bits(),
@@559@@);
    assert_eq!(
        super::atan2(1.0, f64::INFINITY).to_bits(),
@@560@@);
    assert_eq!(
        super::atan2(1.7976931348623157E308, 4.9E-324).to_bits(),
@@561@@);
    assert_eq!(super::atan2(1.0E300, 1.0E300).to_bits(), @@562@@);
    assert_eq!(
        super::atan2(1.0E-300, 1.0E-300).to_bits(),
@@563@@);
    assert_eq!(
        super::atan2(12345.678, 98765.432).to_bits(),
@@564@@);
    assert_eq!(super::atan2(0.5, 0.5).to_bits(), @@565@@);
    assert_eq!(super::atan2(-0.5, 0.5).to_bits(), @@566@@);
    assert_eq!(super::atan2(0.5, -0.5).to_bits(), @@567@@);
    assert_eq!(super::atan2(-0.5, -0.5).to_bits(), @@568@@);
    assert_eq!(super::atan2(0.9, 0.1).to_bits(), @@569@@);
    assert_eq!(super::atan2(0.1, 0.9).to_bits(), @@570@@);
    assert_eq!(super::atan2(10.0, 1.0).to_bits(), @@571@@);
    assert_eq!(super::atan2(1.0, 10.0).to_bits(), @@572@@);
    assert_eq!(super::atan2(-10.0, 1.0).to_bits(), @@573@@);
    assert_eq!(super::atan2(1.0, -10.0).to_bits(), @@574@@);
    assert_eq!(super::atan2(1000.0, -0.001).to_bits(), @@575@@);
    assert_eq!(super::atan2(-0.001, 1000.0).to_bits(), @@576@@);
    assert_eq!(
        super::atan2(0.0, f64::NEG_INFINITY).to_bits(),
@@577@@);
    assert_eq!(
        super::atan2(f64::NEG_INFINITY, 0.0).to_bits(),
@@578@@);
}

#[test]
fn golden_13() {
    // invSqrt/fastInvSqrt/fastInvCubeRoot
    assert_eq!(super::inv_sqrt(0.25f32).to_bits(), @@579@@);
    assert_eq!(super::inv_sqrt(2.0f32).to_bits(), @@580@@);
    assert_eq!(super::inv_sqrt(100.0f32).to_bits(), @@581@@);
    assert_eq!(super::inv_sqrt(1.0f32).to_bits(), @@582@@);
    assert_eq!(super::inv_sqrt(4.0f32).to_bits(), @@583@@);
    assert_eq!(super::inv_sqrt(3.4028235E38f32).to_bits(), @@584@@);
    assert_eq!(super::inv_sqrt(1.4E-45f32).to_bits(), @@585@@);
    assert_eq!(super::inv_sqrt(0.0f32).to_bits(), @@586@@);
    assert_eq!(super::inv_sqrt(-1.0f32).to_bits(), @@587@@);
    assert_eq!(super::inv_sqrt(f32::INFINITY).to_bits(), @@588@@);
    assert_eq!(super::inv_sqrt_f64(0.25).to_bits(), @@589@@);
    assert_eq!(super::inv_sqrt_f64(2.0).to_bits(), @@590@@);
    assert_eq!(super::inv_sqrt_f64(100.0).to_bits(), @@591@@);
    assert_eq!(super::inv_sqrt_f64(1.0).to_bits(), @@592@@);
    assert_eq!(super::inv_sqrt_f64(4.0).to_bits(), @@593@@);
    assert_eq!(
        super::inv_sqrt_f64(1.7976931348623157E308).to_bits(),
@@594@@);
    assert_eq!(super::inv_sqrt_f64(4.9E-324).to_bits(), @@595@@);
    assert_eq!(super::inv_sqrt_f64(0.0).to_bits(), @@596@@);
    assert_eq!(super::inv_sqrt_f64(-1.0).to_bits(), @@597@@);
    assert_eq!(super::inv_sqrt_f64(1.0E-300).to_bits(), @@598@@);
    assert_eq!(
        super::inv_sqrt_f64(f64::INFINITY).to_bits(),
@@599@@);
    assert_eq!(super::fast_inv_sqrt(0.25).to_bits(), @@600@@);
    assert_eq!(super::fast_inv_sqrt(2.0).to_bits(), @@601@@);
    assert_eq!(super::fast_inv_sqrt(100.0).to_bits(), @@602@@);
    assert_eq!(super::fast_inv_sqrt(1.0).to_bits(), @@603@@);
    assert_eq!(super::fast_inv_sqrt(4.0).to_bits(), @@604@@);
    assert_eq!(
        super::fast_inv_sqrt(1.7976931348623157E308).to_bits(),
@@605@@);
    assert_eq!(super::fast_inv_sqrt(4.9E-324).to_bits(), @@606@@);
    assert_eq!(super::fast_inv_sqrt(0.0).to_bits(), @@607@@);
    assert_eq!(super::fast_inv_sqrt(-1.0).to_bits(), @@608@@);
    assert_eq!(super::fast_inv_sqrt(1.0E-300).to_bits(), @@609@@);
    assert_eq!(super::fast_inv_sqrt(1.0E300).to_bits(), @@610@@);
    assert_eq!(super::fast_inv_sqrt(0.5).to_bits(), @@611@@);
    assert_eq!(super::fast_inv_sqrt(0.1).to_bits(), @@612@@);
    assert_eq!(
        super::fast_inv_sqrt(12345.678).to_bits(),
@@613@@);
    assert_eq!(super::fast_inv_sqrt(f64::NAN).to_bits(), @@614@@);
    assert_eq!(super::fast_inv_cube_root(1.0f32).to_bits(), @@615@@);
    assert_eq!(super::fast_inv_cube_root(8.0f32).to_bits(), @@616@@);
    assert_eq!(super::fast_inv_cube_root(27.0f32).to_bits(), @@617@@);
    assert_eq!(super::fast_inv_cube_root(0.5f32).to_bits(), @@618@@);
    assert_eq!(super::fast_inv_cube_root(100.0f32).to_bits(), @@619@@);
    assert_eq!(super::fast_inv_cube_root(0.0f32).to_bits(), @@620@@);
    assert_eq!(super::fast_inv_cube_root(2.0f32).to_bits(), @@621@@);
    assert_eq!(
        super::fast_inv_cube_root(3.4028235E38f32).to_bits(),
@@622@@);
    assert_eq!(super::fast_inv_cube_root(1.4E-45f32).to_bits(), @@623@@);
    assert_eq!(super::fast_inv_cube_root(-8.0f32).to_bits(), @@624@@);
    assert_eq!(super::fast_inv_cube_root(-1.0f32).to_bits(), @@625@@);
    assert_eq!(super::fast_inv_cube_root(f32::NAN).to_bits(), @@626@@);
    assert_eq!(
        super::fast_inv_cube_root(f32::INFINITY).to_bits(),
@@627@@);
}

#[test]
fn golden_14() {
    // hsvToArgb
    assert_eq!(super::hsv_to_argb(0.0f32, 1.0f32, 1.0f32, 0), @@628@@);
    assert_eq!(super::hsv_to_argb(0.5f32, 1.0f32, 1.0f32, 0), @@629@@);
    assert_eq!(super::hsv_to_argb(0.33f32, 0.5f32, 0.7f32, 0), @@630@@);
    assert_eq!(super::hsv_to_argb(1.0f32, 1.0f32, 1.0f32, 0), @@631@@);
    assert_eq!(super::hsv_to_argb(0.25f32, 1.0f32, 1.0f32, 0), @@632@@);
    assert_eq!(super::hsv_to_argb(0.75f32, 1.0f32, 1.0f32, 0), @@633@@);
    assert_eq!(super::hsv_to_argb(0.0f32, 0.0f32, 1.0f32, 0), @@634@@);
    assert_eq!(super::hsv_to_argb(0.0f32, 1.0f32, 0.0f32, 0), @@635@@);
    assert_eq!(
        super::hsv_to_argb(0.08333f32, 1.0f32, 1.0f32, 0),
@@636@@);
    assert_eq!(
        super::hsv_to_argb(0.16666f32, 1.0f32, 1.0f32, 0),
@@637@@);
    assert_eq!(super::hsv_to_argb(0.41666f32, 1.0f32, 1.0f32, 0), @@638@@);
    assert_eq!(super::hsv_to_argb(0.58333f32, 1.0f32, 1.0f32, 0), @@639@@);
    assert_eq!(
        super::hsv_to_argb(0.83333f32, 1.0f32, 1.0f32, 0),
@@640@@);
    assert_eq!(
        super::hsv_to_argb(0.91666f32, 1.0f32, 1.0f32, 0),
@@641@@);
    assert_eq!(super::hsv_to_argb(0.5f32, 0.5f32, 0.5f32, 0), @@642@@);
    assert_eq!(
        super::hsv_to_argb(0.123f32, 0.456f32, 0.789f32, 0),
@@643@@);
    assert_eq!(super::hsv_to_argb(6.0f32, 1.0f32, 1.0f32, 0), @@644@@);
    assert_eq!(super::hsv_to_argb(0.0f32, 1.0f32, 1.0f32, 0), @@645@@);
    assert_eq!(super::hsv_to_argb(0.5, 1.0, 1.0, 128), @@646@@);
    assert_eq!(super::hsv_to_argb(0.33, 0.5, 0.7, 255), @@647@@);
    assert_eq!(super::hsv_to_rgb(0.5, 1.0, 1.0), @@648@@);
}

#[test]
fn golden_15() {
    // misc math
    assert_eq!(
        super::catmullrom(0.5, 0.0, 1.0, 2.0, 3.0).to_bits(),
@@649@@);
    assert_eq!(
        super::catmullrom(0.0, 0.0, 1.0, 2.0, 3.0).to_bits(),
@@650@@);
    assert_eq!(
        super::catmullrom(1.0, 0.0, 1.0, 2.0, 3.0).to_bits(),
@@651@@);
    assert_eq!(super::smoothstep(0.0).to_bits(), @@652@@);
    assert_eq!(super::smoothstep(0.5).to_bits(), @@653@@);
    assert_eq!(super::smoothstep(1.0).to_bits(), @@654@@);
    assert_eq!(super::smoothstep(-0.5).to_bits(), @@655@@);
    assert_eq!(super::smoothstep(2.0).to_bits(), @@656@@);
    assert_eq!(super::smoothstep(f64::NAN).to_bits(), @@657@@);
    assert_eq!(
        super::smoothstep_derivative(0.0).to_bits(),
@@658@@);
    assert_eq!(
        super::smoothstep_derivative(0.5).to_bits(),
@@659@@);
    assert_eq!(
        super::smoothstep_derivative(1.0).to_bits(),
@@660@@);
    assert_eq!(
        super::smoothstep_derivative(-0.5).to_bits(),
@@661@@);
    assert_eq!(
        super::smoothstep_derivative(2.0).to_bits(),
@@662@@);
    assert_eq!(super::sign(0.0), @@663@@);
    assert_eq!(super::sign(-0.0), @@664@@);
    assert_eq!(super::sign(1.0), @@665@@);
    assert_eq!(super::sign(-1.0), @@666@@);
    assert_eq!(super::sign(0.5), @@667@@);
    assert_eq!(super::sign(-0.5), @@668@@);
    assert_eq!(super::sign(f64::NAN), @@669@@);
    assert_eq!(super::sign(f64::INFINITY), @@670@@);
    assert_eq!(super::rot_lerp(0.5, 10.0, 350.0).to_bits(), @@671@@);
    assert_eq!(super::rot_lerp(0.5, 350.0, 10.0).to_bits(), @@672@@);
    assert_eq!(
        super::rot_lerp_f64(0.5, 10.0, 350.0).to_bits(),
@@673@@);
    assert_eq!(super::rot_lerp_rad(0.5, 0.0, 3.5).to_bits(), @@674@@);
    assert_eq!(super::rot_lerp_rad(0.5, 0.0, -3.5).to_bits(), @@675@@);
    assert_eq!(
        super::rot_lerp_rad(0.5, 1.0, 1.0 + 6.3).to_bits(),
@@676@@);
    assert_eq!(super::triangle_wave(1.0, 4.0).to_bits(), @@677@@);
    assert_eq!(super::triangle_wave(2.0, 4.0).to_bits(), @@678@@);
    assert_eq!(super::triangle_wave(3.0, 4.0).to_bits(), @@679@@);
    assert_eq!(super::triangle_wave(4.0, 4.0).to_bits(), @@680@@);
    assert_eq!(super::triangle_wave(-1.0, 4.0).to_bits(), @@681@@);
    assert_eq!(super::square_i32(5), @@682@@);
    assert_eq!(super::square_i32(-5), @@683@@);
    assert_eq!(super::square_i64(3037000499i64), @@684@@);
    assert_eq!(super::square_i64(-3037000499i64), @@685@@);
    assert_eq!(super::square_f32(1.5).to_bits(), @@686@@);
    assert_eq!(super::cube(2.0).to_bits(), @@687@@);
    assert_eq!(super::square_f64(1.5).to_bits(), @@688@@);
    assert_eq!(
        super::clamped_map(5.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
@@689@@);
    assert_eq!(
        super::map(5.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
@@690@@);
    assert_eq!(
        super::inverse_lerp(5.0, 0.0, 10.0).to_bits(),
@@691@@);
    assert_eq!(
        super::clamped_map(-5.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
@@692@@);
    assert_eq!(
        super::map(-5.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
@@693@@);
    assert_eq!(
        super::inverse_lerp(-5.0, 0.0, 10.0).to_bits(),
@@694@@);
    assert_eq!(
        super::clamped_map(15.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
@@695@@);
    assert_eq!(
        super::map(15.0, 0.0, 10.0, 100.0, 200.0).to_bits(),
@@696@@);
    assert_eq!(
        super::inverse_lerp(15.0, 0.0, 10.0).to_bits(),
@@697@@);
    assert_eq!(
        super::clamped_map(5.0, 10.0, 0.0, 100.0, 200.0).to_bits(),
@@698@@);
    assert_eq!(
        super::map(5.0, 10.0, 0.0, 100.0, 200.0).to_bits(),
@@699@@);
    assert_eq!(
        super::inverse_lerp(5.0, 10.0, 0.0).to_bits(),
@@700@@);
    assert_eq!(
        super::clamped_map_f32(5.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
@@701@@);
    assert_eq!(
        super::map_f32(5.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
@@702@@);
    assert_eq!(
        super::clamped_map_f32(-5.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
@@703@@);
    assert_eq!(
        super::map_f32(-5.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
@@704@@);
    assert_eq!(
        super::clamped_map_f32(15.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
@@705@@);
    assert_eq!(
        super::map_f32(15.0f32, 0.0f32, 10.0f32, 100.0f32, 200.0f32).to_bits(),
@@706@@);
}

#[test]
fn golden_16() {
    // length/quantize/ceilDiv/roundToward
    assert_eq!(
        super::length_squared(3.0, 4.0).to_bits(),
@@707@@);
    assert_eq!(super::length(3.0, 4.0).to_bits(), @@708@@);
    assert_eq!(super::length_f32(3.0, 4.0).to_bits(), @@709@@);
    assert_eq!(super::length_f32(1.0E20f32, 1.0f32).to_bits(), @@710@@);
    assert_eq!(
        super::length_f32(1.0000007f32, 1.5000008f32).to_bits(),
@@711@@);
    assert_eq!(
        super::length_squared_xyz(1.0, 2.0, 2.0).to_bits(),
@@712@@);
    assert_eq!(
        super::length_xyz(1.0, 2.0, 2.0).to_bits(),
@@713@@);
    assert_eq!(
        super::length_squared_xyz_f32(1.0, 2.0, 2.0).to_bits(),
@@714@@);
    assert_eq!(super::quantize(7.5, 4), @@715@@);
    assert_eq!(super::quantize(-7.5, 4), @@716@@);
    assert_eq!(super::quantize(100.0, 16), @@717@@);
    assert_eq!(super::quantize(-100.0, 16), @@718@@);
    assert_eq!(super::positive_ceil_div(7, 3), @@719@@);
    assert_eq!(super::round_toward(7, 3), @@720@@);
    assert_eq!(super::positive_ceil_div(-7, 3), @@721@@);
    assert_eq!(super::round_toward(-7, 3), @@722@@);
    assert_eq!(super::positive_ceil_div(7, -3), @@723@@);
    assert_eq!(super::round_toward(7, -3), @@724@@);
    assert_eq!(super::positive_ceil_div(-7, -3), @@725@@);
    assert_eq!(super::round_toward(-7, -3), @@726@@);
    assert_eq!(super::positive_ceil_div(0, 5), @@727@@);
    assert_eq!(super::round_toward(0, 5), @@728@@);
    assert_eq!(super::positive_ceil_div(-2147483648, 2), @@729@@);
    assert_eq!(super::round_toward(-2147483648, 2), @@730@@);
    assert_eq!(super::positive_ceil_div(2147483647, 3), @@731@@);
    assert_eq!(super::round_toward(2147483647, 3), @@732@@);
    assert_eq!(super::positive_ceil_div(5, 5), @@733@@);
    assert_eq!(super::round_toward(5, 5), @@734@@);
    assert_eq!(super::positive_ceil_div(-5, 5), @@735@@);
    assert_eq!(super::round_toward(-5, 5), @@736@@);
    assert_eq!(super::positive_ceil_div_i64(7i64, 3i64), @@737@@);
    assert_eq!(super::round_toward_i64(7i64, 3i64), @@738@@);
    assert_eq!(super::positive_ceil_div_i64(-7i64, 3i64), @@739@@);
    assert_eq!(super::round_toward_i64(-7i64, 3i64), @@740@@);
    assert_eq!(super::positive_ceil_div_i64(7i64, -3i64), @@741@@);
    assert_eq!(super::round_toward_i64(7i64, -3i64), @@742@@);
    assert_eq!(super::positive_ceil_div_i64(-7i64, -3i64), @@743@@);
    assert_eq!(super::round_toward_i64(-7i64, -3i64), @@744@@);
    assert_eq!(
        super::positive_ceil_div_i64(-9223372036854775808i64, 2i64),
@@745@@);
    assert_eq!(
        super::round_toward_i64(-9223372036854775808i64, 2i64),
@@746@@);
    assert_eq!(
        super::positive_ceil_div_i64(9223372036854775807i64, 3i64),
@@747@@);
    assert_eq!(
        super::round_toward_i64(9223372036854775807i64, 3i64),
@@748@@);
    assert_eq!(super::positive_ceil_div_i64(5i64, 5i64), @@749@@);
    assert_eq!(super::round_toward_i64(5i64, 5i64), @@750@@);
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
    assert_eq!(r1.next_int(), @@751@@);
    assert_eq!(r1.next_int(), @@752@@);
    assert_eq!(r1.next_int(), @@753@@);
    assert_eq!(r1.next_int(), @@754@@);
    assert_eq!(r1.next_int(), @@755@@);
    assert_eq!(r1.next_int(), @@756@@);
    assert_eq!(r1.next_int(), @@757@@);
    assert_eq!(r1.next_int(), @@758@@);
    assert_eq!(r1.next_int_bound(100), @@759@@);
    assert_eq!(r1.next_int_bound(100), @@760@@);
    assert_eq!(r1.next_int_bound(100), @@761@@);
    assert_eq!(r1.next_int_bound(100), @@762@@);
    assert_eq!(r1.next_int_bound(100), @@763@@);
    assert_eq!(r1.next_int_bound(5), @@764@@);
    assert_eq!(r1.next_int_bound(5), @@765@@);
    assert_eq!(r1.next_int_bound(5), @@766@@);
    assert_eq!(r1.next_int_bound(5), @@767@@);
    assert_eq!(r1.next_int_bound(5), @@768@@);
    assert_eq!(r1.next_float().to_bits(), @@769@@);
    assert_eq!(r1.next_float().to_bits(), @@770@@);
    assert_eq!(r1.next_float().to_bits(), @@771@@);
    assert_eq!(r1.next_float().to_bits(), @@772@@);
    assert_eq!(r1.next_float().to_bits(), @@773@@);
    assert_eq!(r1.next_double().to_bits(), @@774@@);
    assert_eq!(r1.next_double().to_bits(), @@775@@);
    assert_eq!(r1.next_double().to_bits(), @@776@@);
    assert_eq!(r1.next_double().to_bits(), @@777@@);
    assert_eq!(r1.next_double().to_bits(), @@778@@);
    assert_eq!(r1.next_long(), @@779@@);
    assert_eq!(r1.next_long(), @@780@@);
    assert_eq!(r1.next_long(), @@781@@);
    assert_eq!(r1.next_long(), @@782@@);
    assert_eq!(r1.next_long(), @@783@@);
    assert_eq!(r1.next_boolean(), @@784@@);
    assert_eq!(r1.next_boolean(), @@785@@);
    assert_eq!(r1.next_boolean(), @@786@@);
    assert_eq!(r1.next_boolean(), @@787@@);
    assert_eq!(r1.next_boolean(), @@788@@);
    assert_eq!(r1.next_gaussian().to_bits(), @@789@@);
    assert_eq!(r1.next_gaussian().to_bits(), @@790@@);
    assert_eq!(r1.next_gaussian().to_bits(), @@791@@);
    assert_eq!(r1.next_gaussian().to_bits(), @@792@@);
    assert_eq!(r1.next_gaussian().to_bits(), @@793@@);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), @@794@@);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), @@795@@);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), @@796@@);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), @@797@@);
    assert_eq!(r1.triangle_f64(1.0, 2.0).to_bits(), @@798@@);
    assert_eq!(r1.next_int_between_inclusive(10, 20), @@799@@);
    assert_eq!(r1.next_int_between_inclusive(10, 20), @@800@@);
    assert_eq!(r1.next_int_between_inclusive(10, 20), @@801@@);
    assert_eq!(r42.next_int(), @@802@@);
    assert_eq!(r42.next_int(), @@803@@);
    assert_eq!(r42.next_int(), @@804@@);
    assert_eq!(r42.next_int(), @@805@@);
    assert_eq!(r42.next_int(), @@806@@);
    assert_eq!(r42.next_int(), @@807@@);
    assert_eq!(r42.next_int(), @@808@@);
    assert_eq!(r42.next_int(), @@809@@);
    assert_eq!(r42.next_int_bound(100), @@810@@);
    assert_eq!(r42.next_int_bound(100), @@811@@);
    assert_eq!(r42.next_int_bound(100), @@812@@);
    assert_eq!(r42.next_int_bound(100), @@813@@);
    assert_eq!(r42.next_int_bound(100), @@814@@);
    assert_eq!(r42.next_int_bound(5), @@815@@);
    assert_eq!(r42.next_int_bound(5), @@816@@);
    assert_eq!(r42.next_int_bound(5), @@817@@);
    assert_eq!(r42.next_int_bound(5), @@818@@);
    assert_eq!(r42.next_int_bound(5), @@819@@);
    assert_eq!(r42.next_float().to_bits(), @@820@@);
    assert_eq!(r42.next_float().to_bits(), @@821@@);
    assert_eq!(r42.next_float().to_bits(), @@822@@);
    assert_eq!(r42.next_float().to_bits(), @@823@@);
    assert_eq!(r42.next_float().to_bits(), @@824@@);
    assert_eq!(r42.next_double().to_bits(), @@825@@);
    assert_eq!(r42.next_double().to_bits(), @@826@@);
    assert_eq!(r42.next_double().to_bits(), @@827@@);
    assert_eq!(r42.next_double().to_bits(), @@828@@);
    assert_eq!(r42.next_double().to_bits(), @@829@@);
    assert_eq!(r42.next_long(), @@830@@);
    assert_eq!(r42.next_long(), @@831@@);
    assert_eq!(r42.next_long(), @@832@@);
    assert_eq!(r42.next_long(), @@833@@);
    assert_eq!(r42.next_long(), @@834@@);
    assert_eq!(r42.next_boolean(), @@835@@);
    assert_eq!(r42.next_boolean(), @@836@@);
    assert_eq!(r42.next_boolean(), @@837@@);
    assert_eq!(r42.next_boolean(), @@838@@);
    assert_eq!(r42.next_boolean(), @@839@@);
    assert_eq!(r42.next_gaussian().to_bits(), @@840@@);
    assert_eq!(r42.next_gaussian().to_bits(), @@841@@);
    assert_eq!(r42.next_gaussian().to_bits(), @@842@@);
    assert_eq!(r42.next_gaussian().to_bits(), @@843@@);
    assert_eq!(r42.next_gaussian().to_bits(), @@844@@);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), @@845@@);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), @@846@@);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), @@847@@);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), @@848@@);
    assert_eq!(r42.triangle_f64(1.0, 2.0).to_bits(), @@849@@);
    assert_eq!(r42.next_int_between_inclusive(10, 20), @@850@@);
    assert_eq!(r42.next_int_between_inclusive(10, 20), @@851@@);
    assert_eq!(r42.next_int_between_inclusive(10, 20), @@852@@);
    assert_eq!(r123456789.next_int(), @@853@@);
    assert_eq!(r123456789.next_int(), @@854@@);
    assert_eq!(r123456789.next_int(), @@855@@);
    assert_eq!(r123456789.next_int(), @@856@@);
    assert_eq!(r123456789.next_int(), @@857@@);
    assert_eq!(r123456789.next_int(), @@858@@);
    assert_eq!(r123456789.next_int(), @@859@@);
    assert_eq!(r123456789.next_int(), @@860@@);
    assert_eq!(r123456789.next_int_bound(100), @@861@@);
    assert_eq!(r123456789.next_int_bound(100), @@862@@);
    assert_eq!(r123456789.next_int_bound(100), @@863@@);
    assert_eq!(r123456789.next_int_bound(100), @@864@@);
    assert_eq!(r123456789.next_int_bound(100), @@865@@);
    assert_eq!(r123456789.next_int_bound(5), @@866@@);
    assert_eq!(r123456789.next_int_bound(5), @@867@@);
    assert_eq!(r123456789.next_int_bound(5), @@868@@);
    assert_eq!(r123456789.next_int_bound(5), @@869@@);
    assert_eq!(r123456789.next_int_bound(5), @@870@@);
    assert_eq!(r123456789.next_float().to_bits(), @@871@@);
    assert_eq!(r123456789.next_float().to_bits(), @@872@@);
    assert_eq!(r123456789.next_float().to_bits(), @@873@@);
    assert_eq!(r123456789.next_float().to_bits(), @@874@@);
    assert_eq!(r123456789.next_float().to_bits(), @@875@@);
    assert_eq!(r123456789.next_double().to_bits(), @@876@@);
    assert_eq!(r123456789.next_double().to_bits(), @@877@@);
    assert_eq!(r123456789.next_double().to_bits(), @@878@@);
    assert_eq!(r123456789.next_double().to_bits(), @@879@@);
    assert_eq!(r123456789.next_double().to_bits(), @@880@@);
    assert_eq!(r123456789.next_long(), @@881@@);
    assert_eq!(r123456789.next_long(), @@882@@);
    assert_eq!(r123456789.next_long(), @@883@@);
    assert_eq!(r123456789.next_long(), @@884@@);
    assert_eq!(r123456789.next_long(), @@885@@);
    assert_eq!(r123456789.next_boolean(), @@886@@);
    assert_eq!(r123456789.next_boolean(), @@887@@);
    assert_eq!(r123456789.next_boolean(), @@888@@);
    assert_eq!(r123456789.next_boolean(), @@889@@);
    assert_eq!(r123456789.next_boolean(), @@890@@);
    assert_eq!(r123456789.next_gaussian().to_bits(), @@891@@);
    assert_eq!(r123456789.next_gaussian().to_bits(), @@892@@);
    assert_eq!(r123456789.next_gaussian().to_bits(), @@893@@);
    assert_eq!(r123456789.next_gaussian().to_bits(), @@894@@);
    assert_eq!(r123456789.next_gaussian().to_bits(), @@895@@);
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
@@896@@);
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
@@897@@);
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
@@898@@);
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
@@899@@);
    assert_eq!(
        r123456789.triangle_f64(1.0, 2.0).to_bits(),
@@900@@);
    assert_eq!(r123456789.next_int_between_inclusive(10, 20), @@901@@);
    assert_eq!(r123456789.next_int_between_inclusive(10, 20), @@902@@);
    assert_eq!(r123456789.next_int_between_inclusive(10, 20), @@903@@);
    assert_eq!(rNEG1.next_int(), @@904@@);
    assert_eq!(rNEG1.next_int(), @@905@@);
    assert_eq!(rNEG1.next_int(), @@906@@);
    assert_eq!(rNEG1.next_int(), @@907@@);
    assert_eq!(rNEG1.next_int(), @@908@@);
    assert_eq!(rNEG1.next_int(), @@909@@);
    assert_eq!(rNEG1.next_int(), @@910@@);
    assert_eq!(rNEG1.next_int(), @@911@@);
    assert_eq!(rNEG1.next_int_bound(100), @@912@@);
    assert_eq!(rNEG1.next_int_bound(100), @@913@@);
    assert_eq!(rNEG1.next_int_bound(100), @@914@@);
    assert_eq!(rNEG1.next_int_bound(100), @@915@@);
    assert_eq!(rNEG1.next_int_bound(100), @@916@@);
    assert_eq!(rNEG1.next_int_bound(5), @@917@@);
    assert_eq!(rNEG1.next_int_bound(5), @@918@@);
    assert_eq!(rNEG1.next_int_bound(5), @@919@@);
    assert_eq!(rNEG1.next_int_bound(5), @@920@@);
    assert_eq!(rNEG1.next_int_bound(5), @@921@@);
    assert_eq!(rNEG1.next_float().to_bits(), @@922@@);
    assert_eq!(rNEG1.next_float().to_bits(), @@923@@);
    assert_eq!(rNEG1.next_float().to_bits(), @@924@@);
    assert_eq!(rNEG1.next_float().to_bits(), @@925@@);
    assert_eq!(rNEG1.next_float().to_bits(), @@926@@);
    assert_eq!(rNEG1.next_double().to_bits(), @@927@@);
    assert_eq!(rNEG1.next_double().to_bits(), @@928@@);
    assert_eq!(rNEG1.next_double().to_bits(), @@929@@);
    assert_eq!(rNEG1.next_double().to_bits(), @@930@@);
    assert_eq!(rNEG1.next_double().to_bits(), @@931@@);
    assert_eq!(rNEG1.next_long(), @@932@@);
    assert_eq!(rNEG1.next_long(), @@933@@);
    assert_eq!(rNEG1.next_long(), @@934@@);
    assert_eq!(rNEG1.next_long(), @@935@@);
    assert_eq!(rNEG1.next_long(), @@936@@);
    assert_eq!(rNEG1.next_boolean(), @@937@@);
    assert_eq!(rNEG1.next_boolean(), @@938@@);
    assert_eq!(rNEG1.next_boolean(), @@939@@);
    assert_eq!(rNEG1.next_boolean(), @@940@@);
    assert_eq!(rNEG1.next_boolean(), @@941@@);
    assert_eq!(rNEG1.next_gaussian().to_bits(), @@942@@);
    assert_eq!(rNEG1.next_gaussian().to_bits(), @@943@@);
    assert_eq!(rNEG1.next_gaussian().to_bits(), @@944@@);
    assert_eq!(rNEG1.next_gaussian().to_bits(), @@945@@);
    assert_eq!(rNEG1.next_gaussian().to_bits(), @@946@@);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), @@947@@);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), @@948@@);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), @@949@@);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), @@950@@);
    assert_eq!(rNEG1.triangle_f64(1.0, 2.0).to_bits(), @@951@@);
    assert_eq!(rNEG1.next_int_between_inclusive(10, 20), @@952@@);
    assert_eq!(rNEG1.next_int_between_inclusive(10, 20), @@953@@);
    assert_eq!(rNEG1.next_int_between_inclusive(10, 20), @@954@@);
    assert_eq!(rNEG9223372036854775808.next_int(), @@955@@);
    assert_eq!(rNEG9223372036854775808.next_int(), @@956@@);
    assert_eq!(rNEG9223372036854775808.next_int(), @@957@@);
    assert_eq!(rNEG9223372036854775808.next_int(), @@958@@);
    assert_eq!(rNEG9223372036854775808.next_int(), @@959@@);
    assert_eq!(rNEG9223372036854775808.next_int(), @@960@@);
    assert_eq!(rNEG9223372036854775808.next_int(), @@961@@);
    assert_eq!(rNEG9223372036854775808.next_int(), @@962@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), @@963@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), @@964@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), @@965@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), @@966@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(100), @@967@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), @@968@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), @@969@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), @@970@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), @@971@@);
    assert_eq!(rNEG9223372036854775808.next_int_bound(5), @@972@@);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), @@973@@);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), @@974@@);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), @@975@@);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), @@976@@);
    assert_eq!(rNEG9223372036854775808.next_float().to_bits(), @@977@@);
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
@@978@@);
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
@@979@@);
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
@@980@@);
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
@@981@@);
    assert_eq!(
        rNEG9223372036854775808.next_double().to_bits(),
@@982@@);
    assert_eq!(rNEG9223372036854775808.next_long(), @@983@@);
    assert_eq!(rNEG9223372036854775808.next_long(), @@984@@);
    assert_eq!(rNEG9223372036854775808.next_long(), @@985@@);
    assert_eq!(rNEG9223372036854775808.next_long(), @@986@@);
    assert_eq!(rNEG9223372036854775808.next_long(), @@987@@);
    assert_eq!(rNEG9223372036854775808.next_boolean(), @@988@@);
    assert_eq!(rNEG9223372036854775808.next_boolean(), @@989@@);
    assert_eq!(rNEG9223372036854775808.next_boolean(), @@990@@);
    assert_eq!(rNEG9223372036854775808.next_boolean(), @@991@@);
    assert_eq!(rNEG9223372036854775808.next_boolean(), @@992@@);
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
@@993@@);
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
@@994@@);
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
@@995@@);
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
@@996@@);
    assert_eq!(
        rNEG9223372036854775808.next_gaussian().to_bits(),
@@997@@);
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
@@998@@);
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
@@999@@);
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
@@1000@@);
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
@@1001@@);
    assert_eq!(
        rNEG9223372036854775808.triangle_f64(1.0, 2.0).to_bits(),
@@1002@@);
    assert_eq!(
        rNEG9223372036854775808.next_int_between_inclusive(10, 20),
@@1003@@);
    assert_eq!(
        rNEG9223372036854775808.next_int_between_inclusive(10, 20),
@@1004@@);
    assert_eq!(
        rNEG9223372036854775808.next_int_between_inclusive(10, 20),
@@1005@@);
    assert_eq!(r244837814047284.next_int(), @@1006@@);
    assert_eq!(r244837814047284.next_int(), @@1007@@);
    assert_eq!(r244837814047284.next_int(), @@1008@@);
    assert_eq!(r244837814047284.next_int(), @@1009@@);
    assert_eq!(r244837814047284.next_int(), @@1010@@);
    assert_eq!(r244837814047284.next_int(), @@1011@@);
    assert_eq!(r244837814047284.next_int(), @@1012@@);
    assert_eq!(r244837814047284.next_int(), @@1013@@);
    assert_eq!(r244837814047284.next_int_bound(100), @@1014@@);
    assert_eq!(r244837814047284.next_int_bound(100), @@1015@@);
    assert_eq!(r244837814047284.next_int_bound(100), @@1016@@);
    assert_eq!(r244837814047284.next_int_bound(100), @@1017@@);
    assert_eq!(r244837814047284.next_int_bound(100), @@1018@@);
    assert_eq!(r244837814047284.next_int_bound(5), @@1019@@);
    assert_eq!(r244837814047284.next_int_bound(5), @@1020@@);
    assert_eq!(r244837814047284.next_int_bound(5), @@1021@@);
    assert_eq!(r244837814047284.next_int_bound(5), @@1022@@);
    assert_eq!(r244837814047284.next_int_bound(5), @@1023@@);
    assert_eq!(r244837814047284.next_float().to_bits(), @@1024@@);
    assert_eq!(r244837814047284.next_float().to_bits(), @@1025@@);
    assert_eq!(r244837814047284.next_float().to_bits(), @@1026@@);
    assert_eq!(r244837814047284.next_float().to_bits(), @@1027@@);
    assert_eq!(r244837814047284.next_float().to_bits(), @@1028@@);
    assert_eq!(r244837814047284.next_double().to_bits(), @@1029@@);
    assert_eq!(r244837814047284.next_double().to_bits(), @@1030@@);
    assert_eq!(r244837814047284.next_double().to_bits(), @@1031@@);
    assert_eq!(r244837814047284.next_double().to_bits(), @@1032@@);
    assert_eq!(r244837814047284.next_double().to_bits(), @@1033@@);
    assert_eq!(r244837814047284.next_long(), @@1034@@);
    assert_eq!(r244837814047284.next_long(), @@1035@@);
    assert_eq!(r244837814047284.next_long(), @@1036@@);
    assert_eq!(r244837814047284.next_long(), @@1037@@);
    assert_eq!(r244837814047284.next_long(), @@1038@@);
    assert_eq!(r244837814047284.next_boolean(), @@1039@@);
    assert_eq!(r244837814047284.next_boolean(), @@1040@@);
    assert_eq!(r244837814047284.next_boolean(), @@1041@@);
    assert_eq!(r244837814047284.next_boolean(), @@1042@@);
    assert_eq!(r244837814047284.next_boolean(), @@1043@@);
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
@@1044@@);
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
@@1045@@);
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
@@1046@@);
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
@@1047@@);
    assert_eq!(
        r244837814047284.next_gaussian().to_bits(),
@@1048@@);
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
@@1049@@);
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
@@1050@@);
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
@@1051@@);
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
@@1052@@);
    assert_eq!(
        r244837814047284.triangle_f64(1.0, 2.0).to_bits(),
@@1053@@);
    assert_eq!(r244837814047284.next_int_between_inclusive(10, 20), @@1054@@);
    assert_eq!(r244837814047284.next_int_between_inclusive(10, 20), @@1055@@);
    assert_eq!(r244837814047284.next_int_between_inclusive(10, 20), @@1056@@);
}

#[test]
fn golden_18() {
    let mut r0 = crate::random_source::SingleThreadedRandomSource::new(0i64);
    let mut r1 = crate::random_source::SingleThreadedRandomSource::new(1i64);
    let mut r7 = crate::random_source::SingleThreadedRandomSource::new(7i64);
    let mut r999 = crate::random_source::SingleThreadedRandomSource::new(999i64);
    // Mth RNG helpers
    assert_eq!(super::next_int(&mut r0, 3, 10), @@1057@@);
    assert_eq!(super::next_float(&mut r0, 1.0, 2.0).to_bits(), @@1058@@);
    assert_eq!(
        super::next_double(&mut r0, 1.0, 2.0).to_bits(),
@@1059@@);
    assert_eq!(super::random_between_inclusive(&mut r0, 10, 30), @@1060@@);
    assert_eq!(
        super::random_between(&mut r0, 5.0, 10.0).to_bits(),
@@1061@@);
    assert_eq!(super::normal(&mut r0, 100.0, 5.0).to_bits(), @@1062@@);
    assert_eq!(super::next_int(&mut r1, 3, 10), @@1063@@);
    assert_eq!(super::next_float(&mut r1, 1.0, 2.0).to_bits(), @@1064@@);
    assert_eq!(
        super::next_double(&mut r1, 1.0, 2.0).to_bits(),
@@1065@@);
    assert_eq!(super::random_between_inclusive(&mut r1, 10, 30), @@1066@@);
    assert_eq!(
        super::random_between(&mut r1, 5.0, 10.0).to_bits(),
@@1067@@);
    assert_eq!(super::normal(&mut r1, 100.0, 5.0).to_bits(), @@1068@@);
    assert_eq!(super::next_int(&mut r7, 3, 10), @@1069@@);
    assert_eq!(super::next_float(&mut r7, 1.0, 2.0).to_bits(), @@1070@@);
    assert_eq!(
        super::next_double(&mut r7, 1.0, 2.0).to_bits(),
@@1071@@);
    assert_eq!(super::random_between_inclusive(&mut r7, 10, 30), @@1072@@);
    assert_eq!(
        super::random_between(&mut r7, 5.0, 10.0).to_bits(),
@@1073@@);
    assert_eq!(super::normal(&mut r7, 100.0, 5.0).to_bits(), @@1074@@);
    assert_eq!(super::next_int(&mut r999, 3, 10), @@1075@@);
    assert_eq!(super::next_float(&mut r999, 1.0, 2.0).to_bits(), @@1076@@);
    assert_eq!(
        super::next_double(&mut r999, 1.0, 2.0).to_bits(),
@@1077@@);
    assert_eq!(super::random_between_inclusive(&mut r999, 10, 30), @@1078@@);
    assert_eq!(
        super::random_between(&mut r999, 5.0, 10.0).to_bits(),
@@1079@@);
    assert_eq!(super::normal(&mut r999, 100.0, 5.0).to_bits(), @@1080@@);
}

#[test]
fn golden_19() {
    // wobble / createInsecureUUID
    assert_eq!(super::wobble(1.0).to_bits(), @@1081@@);
    assert_eq!(super::wobble(0.0).to_bits(), @@1082@@);
    assert_eq!(super::wobble(-1.0).to_bits(), @@1083@@);
    assert_eq!(super::wobble(123.456).to_bits(), @@1084@@);
    assert_eq!(super::wobble(-987.654).to_bits(), @@1085@@);
    assert_eq!(super::wobble(3.0E-4).to_bits(), @@1086@@);
    assert_eq!(super::wobble(-3.0E-4).to_bits(), @@1087@@);
    assert_eq!(
        super::create_insecure_uuid(&mut crate::random_source::SingleThreadedRandomSource::new(
            1i64
        ))
        .most,
@@1088@@);
    assert_eq!(
        super::create_insecure_uuid(&mut crate::random_source::SingleThreadedRandomSource::new(
            1i64
        ))
        .least,
@@1089@@);
    assert_eq!(
        super::create_insecure_uuid(&mut crate::random_source::SingleThreadedRandomSource::new(
            42i64
        ))
        .most,
@@1090@@);
    assert_eq!(
        super::create_insecure_uuid(&mut crate::random_source::SingleThreadedRandomSource::new(
            42i64
        ))
        .least,
@@1091@@);
}

#[test]
fn golden_20() {
    // binarySearch
    assert_eq!(super::binary_search(0, 10, |x| x * x > 50), @@1092@@);
    assert_eq!(super::binary_search(0, 10, |x| x >= 3), @@1093@@);
    assert_eq!(super::binary_search(0, 10, |x| x > 100), @@1094@@);
    assert_eq!(super::binary_search(0, 0, |x| x >= 0), @@1095@@);
    assert_eq!(super::binary_search(-10, 10, |x| x >= 0), @@1096@@);
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
@@1097@@);
    assert_eq!(
        super::out_from_origin(0, 0, 5).collect::<Vec<_>>(),
@@1098@@);
    assert_eq!(
        super::out_from_origin(10, 0, 20).collect::<Vec<_>>(),
@@1099@@);
    assert_eq!(
        super::out_from_origin(5, 0, 5).collect::<Vec<_>>(),
@@1100@@);
    assert_eq!(
        super::out_from_origin(-5, 0, 10).collect::<Vec<_>>(),
@@1101@@);
    assert_eq!(
        super::out_from_origin(20, 0, 10).collect::<Vec<_>>(),
@@1102@@);
    assert_eq!(
        super::out_from_origin(0, 0, 1).collect::<Vec<_>>(),
@@1103@@);
    assert_eq!(super::out_from_origin(7, 7, 7).collect::<Vec<_>>(), @@1104@@);
    assert_eq!(
        super::out_from_origin(0, -10, -5).collect::<Vec<_>>(),
@@1105@@);
    assert_eq!(
        super::out_from_origin(3, 3, 9).collect::<Vec<_>>(),
@@1106@@);
    assert_eq!(
        super::out_from_origin_with_step(0, -5, 5, 2).collect::<Vec<_>>(),
@@1107@@);
    assert_eq!(
        super::out_from_origin_with_step(0, 0, 9, 3).collect::<Vec<_>>(),
@@1108@@);
    assert_eq!(
        super::out_from_origin_with_step(5, 0, 20, 4).collect::<Vec<_>>(),
@@1109@@);
    assert_eq!(
        super::out_from_origin_with_step(0, 0, 5, 1).collect::<Vec<_>>(),
@@1110@@);
    assert_eq!(
        super::out_from_origin_with_step(2, 0, 20, 2).collect::<Vec<_>>(),
@@1111@@);
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
@@1112@@);
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
@@1113@@);
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
@@1114@@);
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
@@1115@@);
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
@@1116@@);
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
@@1117@@);
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
@@1118@@);
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
@@1119@@);
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
@@1120@@);
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
@@1121@@);
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
@@1122@@);
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
@@1123@@);
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
@@1124@@);
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
@@1125@@);
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
@@1126@@);
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
@@1127@@);
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
@@1128@@);
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
@@1129@@);
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
@@1130@@);
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
@@1131@@);
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
@@1132@@);
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
@@1133@@);
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
@@1134@@);
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
@@1135@@);
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
@@1136@@);
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
@@1137@@);
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
@@1138@@);
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
@@1139@@);
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
@@1140@@);
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
@@1141@@);
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
@@1142@@);
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
@@1143@@);
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
@@1144@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 1,
                denominator: 2
            },
            100
        ),
@@1145@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 5,
                denominator: 3
            },
            100
        ),
@@1146@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 7,
                denominator: 1
            },
            100
        ),
@@1147@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 1,
                denominator: 3
            },
            100
        ),
@@1148@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 2147483647,
                denominator: 2
            },
            100
        ),
@@1149@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 3,
                denominator: 4
            },
            7
        ),
@@1150@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 1,
                denominator: 2
            },
            7
        ),
@@1151@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 5,
                denominator: 3
            },
            7
        ),
@@1152@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 7,
                denominator: 1
            },
            7
        ),
@@1153@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 1,
                denominator: 3
            },
            7
        ),
@@1154@@);
    assert_eq!(
        super::mul_and_truncate(
            &crate::mth_stubs::Fraction {
                numerator: 2147483647,
                denominator: 2
            },
            7
        ),
@@1155@@);
}
