//! Port of `net.minecraft.world.level.levelgen.VerticalAnchor` (interface,
//! 26.2) — the height-anchor value/codec layer.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! levelgen/VerticalAnchor.java`.
//!
//! Java models the anchor as an interface with three record implementors
//! (`Absolute(y)`, `AboveBottom(offset)`, `BelowTop(offset)`) plus the
//! `BOTTOM`/`TOP` singletons (`aboveBottom(0)`/`belowTop(0)`). The port keeps
//! the same discriminated-union shape: `VerticalAnchor` is a struct holding
//! the variant enum, mirroring Java's sealed surface. `resolveY` dispatches on
//! the variant, so the `VerticalAnchor` value is the single field of the struct
//! — matching how the codebase ports sealed Java interfaces as a tagged
//! struct (`GenerationStep`, etc.).
//!
//! ## Codec shape (xor/dispatch)
//!
//! ```java
//! CODEC = Codec.xor(Absolute.CODEC, Codec.xor(AboveBottom.CODEC, BelowTop.CODEC))
//!     .xmap(VerticalAnchor::merge, VerticalAnchor::split);
//! ```
//!
//! The nested xor produces `Either<Absolute, Either<AboveBottom, BelowTop>>`
//! and the `merge`/`split` pair folds/unfolds the variants — an exact,
//! order-sensitive dispatch. Each variant codec is
//! `Codec.intRange(MIN_Y, MAX_Y).fieldOf(name).xmap(ctor, field).codec()`.
//!
//! ## resolveY
//!
//! ```java
//! Absolute.resolveY(ctx)    = y
//! AboveBottom.resolveY(ctx) = ctx.getMinGenY() + offset
//! BelowTop.resolveY(ctx)    = ctx.getGenDepth() - 1 + ctx.getMinGenY() - offset
//! ```
//!
//! All arithmetic is Java-int wrapping (see PORTING.md). `DimensionType.MIN_Y`/
//! `MAX_Y` bound every variant codec's int field inclusively on both decode
//! and encode (the shared `Codec.intRange` flatXMap).

use crate::level::dimension::MAX_Y;
use crate::level::dimension::MIN_Y;
use crate::levelgen::world_generation_context::WorldGenerationContext;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::either::Either;
use rivet_serialization::map_codec;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.VerticalAnchor`.
///
/// A value anchor resolving to a world Y. Java models it as an interface with
/// three record implementors; the port mirrors that sealed surface as a single
/// enum over the three variants (the same shape as `GenerationStep.Decoration`
/// etc.).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAnchor {
    /// `record Absolute(int y)` — an absolute world Y.
    Absolute(i32),
    /// `record AboveBottom(int offset)` — `getMinGenY() + offset`.
    AboveBottom(i32),
    /// `record BelowTop(int offset)` — `getGenDepth() - 1 + getMinGenY() - offset`.
    BelowTop(i32),
}

impl VerticalAnchor {
    /// `VerticalAnchor.BOTTOM` — `aboveBottom(0)`.
    pub const BOTTOM: VerticalAnchor = VerticalAnchor::above_bottom(0);
    /// `VerticalAnchor.TOP` — `belowTop(0)`.
    pub const TOP: VerticalAnchor = VerticalAnchor::below_top(0);

    /// `VerticalAnchor.absolute(int)`.
    pub const fn absolute(y: i32) -> VerticalAnchor {
        VerticalAnchor::Absolute(y)
    }

    /// `VerticalAnchor.aboveBottom(int)`.
    pub const fn above_bottom(offset: i32) -> VerticalAnchor {
        VerticalAnchor::AboveBottom(offset)
    }

    /// `VerticalAnchor.belowTop(int)`.
    pub const fn below_top(offset: i32) -> VerticalAnchor {
        VerticalAnchor::BelowTop(offset)
    }

    /// `VerticalAnchor.bottom()`.
    pub const fn bottom() -> VerticalAnchor {
        VerticalAnchor::BOTTOM
    }

    /// `VerticalAnchor.top()`.
    pub const fn top() -> VerticalAnchor {
        VerticalAnchor::TOP
    }

    /// `VerticalAnchor.resolveY(WorldGenerationContext)` — the absolute Y this
    /// anchor resolves to against the worldgen window.
    pub fn resolve_y(&self, height_accessor: &WorldGenerationContext) -> i32 {
        match self {
            VerticalAnchor::Absolute(y) => *y,
            VerticalAnchor::AboveBottom(offset) => {
                height_accessor.get_min_gen_y().wrapping_add(*offset)
            }
            VerticalAnchor::BelowTop(offset) => height_accessor
                .get_gen_depth()
                .wrapping_sub(1)
                .wrapping_add(height_accessor.get_min_gen_y())
                .wrapping_sub(*offset),
        }
    }
}

impl fmt::Display for VerticalAnchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerticalAnchor::Absolute(y) => write!(f, "{} absolute", y),
            VerticalAnchor::AboveBottom(offset) => write!(f, "{} above bottom", offset),
            VerticalAnchor::BelowTop(offset) => write!(f, "{} below top", offset),
        }
    }
}

/// `VerticalAnchor.CODEC` — the xor-dispatched codec, ops-generic (DFU
/// `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant is a
/// `::<Ops>()` factory).
///
/// `Codec.xor(Absolute.CODEC, Codec.xor(AboveBottom.CODEC, BelowTop.CODEC))
/// .xmap(VerticalAnchor::merge, VerticalAnchor::split)`.
///
/// The tree is rebuilt on every call (the same shape as the sibling
/// `probability_feature_configuration_codec`/`decoration_codec` factories).
/// Java builds its `static final` once; the port keeps the rebuild convention
/// and defers per-Ops caching until a consumer makes the cost meaningful.
pub fn vertical_anchor_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<VerticalAnchor, Ops>>
where
    Ops::Output: Clone,
{
    let absolute = absolute_codec::<Ops>();
    let above_bottom = above_bottom_codec::<Ops>();
    let below_top = below_top_codec::<Ops>();
    // `Codec.xor(AboveBottom.CODEC, BelowTop.CODEC)`.
    let inner_xor = codec::xor::<AboveBottom, BelowTop, Ops>(above_bottom, below_top);
    // `Codec.xor(Absolute.CODEC, inner_xor)`.
    let outer_xor = codec::xor::<Absolute, Either<AboveBottom, BelowTop>, Ops>(absolute, inner_xor);
    // `.xmap(merge, split)`.
    codec::xmap(
        outer_xor,
        Arc::new(|e: &Either<Absolute, Either<AboveBottom, BelowTop>>| {
            // `either.map(identity, Either::unwrap)`.
            match e {
                Either::Left(absolute) => VerticalAnchor::absolute(absolute.0),
                Either::Right(inner) => match inner {
                    Either::Left(above) => VerticalAnchor::above_bottom(above.0),
                    Either::Right(below) => VerticalAnchor::below_top(below.0),
                },
            }
        }),
        Arc::new(|anchor: &VerticalAnchor| match anchor {
            VerticalAnchor::Absolute(y) => Either::left(Absolute(*y)),
            VerticalAnchor::AboveBottom(offset) => {
                Either::right(Either::left(AboveBottom(*offset)))
            }
            VerticalAnchor::BelowTop(offset) => Either::right(Either::right(BelowTop(*offset))),
        }),
    )
}

/// The erased value payload `Absolute.CODEC` decodes/encodes (a wrapper over
/// the raw `y` int, so the xor/xmap typing composes with `VerticalAnchor`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Absolute(i32);
/// The erased value payload `AboveBottom.CODEC` decodes/encodes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AboveBottom(i32);
/// The erased value payload `BelowTop.CODEC` decodes/encodes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BelowTop(i32);

/// `VerticalAnchor.Absolute.CODEC` — `Codec.intRange(MIN_Y, MAX_Y)
/// .fieldOf("absolute").xmap(Absolute::new, Absolute::y).codec()`.
fn absolute_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Absolute, Ops>>
where
    Ops::Output: Clone,
{
    // `Codec.intRange(MIN_Y, MAX_Y).fieldOf("absolute")`.
    let field =
        codec::field_of::<i32, Ops>(codec::int_range::<Ops>(MIN_Y, MAX_Y), "absolute".into());
    // `.xmap(Absolute::new, Absolute::y).codec()`.
    let mapped = map_codec::xmap::<i32, Absolute, Ops>(
        field,
        Arc::new(|y: &i32| Absolute(*y)),
        Arc::new(|a: &Absolute| a.0),
    );
    map_codec::codec_of(mapped)
}

/// `VerticalAnchor.AboveBottom.CODEC`.
fn above_bottom_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<AboveBottom, Ops>>
where
    Ops::Output: Clone,
{
    let field =
        codec::field_of::<i32, Ops>(codec::int_range::<Ops>(MIN_Y, MAX_Y), "above_bottom".into());
    let mapped = map_codec::xmap::<i32, AboveBottom, Ops>(
        field,
        Arc::new(|offset: &i32| AboveBottom(*offset)),
        Arc::new(|a: &AboveBottom| a.0),
    );
    map_codec::codec_of(mapped)
}

/// `VerticalAnchor.BelowTop.CODEC`.
fn below_top_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BelowTop, Ops>>
where
    Ops::Output: Clone,
{
    let field =
        codec::field_of::<i32, Ops>(codec::int_range::<Ops>(MIN_Y, MAX_Y), "below_top".into());
    let mapped = map_codec::xmap::<i32, BelowTop, Ops>(
        field,
        Arc::new(|offset: &i32| BelowTop(*offset)),
        Arc::new(|a: &BelowTop| a.0),
    );
    map_codec::codec_of(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// A `ChunkGenerator` double exposing a fixed worldgen window.
    struct TestGenerator {
        min_y: i32,
        depth: i32,
    }
    impl ChunkGenerator for TestGenerator {
        fn get_min_y(&self) -> i32 {
            self.min_y
        }
        fn get_gen_depth(&self) -> i32 {
            self.depth
        }
    }

    /// A `WorldGenLevel` double over a fixed window.
    struct TestLevel(SimpleLevelHeightAccessor);
    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.0.get_height()
        }
        fn get_min_y(&self) -> i32 {
            self.0.get_min_y()
        }
    }
    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }
    }

    fn context(min_y: i32, height: i32, gen_depth: i32) -> WorldGenerationContext {
        let level = TestLevel(create(min_y, height));
        let generator = TestGenerator {
            min_y,
            depth: gen_depth,
        };
        WorldGenerationContext::new(&generator, &level)
    }

    #[test]
    fn resolve_y_matches_paper_window_arithmetic() {
        // Overworld window: minY -64, height 384 (generator depth 384).
        let ctx = context(-64, 384, 384);
        assert_eq!(VerticalAnchor::absolute(0).resolve_y(&ctx), 0);
        assert_eq!(VerticalAnchor::absolute(-64).resolve_y(&ctx), -64);
        assert_eq!(VerticalAnchor::absolute(319).resolve_y(&ctx), 319);
        assert_eq!(VerticalAnchor::above_bottom(0).resolve_y(&ctx), -64);
        assert_eq!(VerticalAnchor::above_bottom(16).resolve_y(&ctx), -48);
        assert_eq!(VerticalAnchor::below_top(0).resolve_y(&ctx), 319);
        assert_eq!(VerticalAnchor::below_top(1).resolve_y(&ctx), 318);
        assert_eq!(VerticalAnchor::below_top(16).resolve_y(&ctx), 303);
        // BOTTOM/TOP singletons.
        assert_eq!(VerticalAnchor::bottom().resolve_y(&ctx), -64);
        assert_eq!(VerticalAnchor::top().resolve_y(&ctx), 319);
    }

    #[test]
    fn resolve_y_uses_the_min_of_height_and_generator_depth() {
        // A generator that clips the window to depth 100: below-top resolves
        // against getGenDepth() = 100, not the level height.
        let ctx = context(-64, 384, 100);
        assert_eq!(VerticalAnchor::below_top(0).resolve_y(&ctx), 35); // 100-1-64
        assert_eq!(VerticalAnchor::above_bottom(0).resolve_y(&ctx), -64);
    }

    #[test]
    fn resolve_y_wraps_on_extreme_inputs() {
        // Java-int wrapping arithmetic for extreme offsets.
        let ctx = context(-64, 384, 384);
        let anchor = VerticalAnchor::above_bottom(i32::MAX);
        // -64 + 2147483647 wraps to 2147483583.
        assert_eq!(anchor.resolve_y(&ctx), i32::MAX - 64);
        let below = VerticalAnchor::below_top(i32::MAX);
        // 384 - 1 + (-64) - 2147483647 = 319 - 2147483647 = -2147483328.
        assert_eq!(below.resolve_y(&ctx), 319i32.wrapping_sub(i32::MAX));
    }

    #[test]
    fn display_matches_java_to_string() {
        assert_eq!(VerticalAnchor::absolute(42).to_string(), "42 absolute");
        assert_eq!(
            VerticalAnchor::above_bottom(7).to_string(),
            "7 above bottom"
        );
        assert_eq!(VerticalAnchor::below_top(3).to_string(), "3 below top");
    }

    #[test]
    fn codec_round_trips_all_variants() {
        let codec = vertical_anchor_codec::<JsonOps>();
        for anchor in [
            VerticalAnchor::absolute(0),
            VerticalAnchor::absolute(2031),
            VerticalAnchor::absolute(-2032),
            VerticalAnchor::above_bottom(16),
            VerticalAnchor::below_top(1),
            VerticalAnchor::below_top(384),
        ] {
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &anchor)
                .result()
                .expect("encode should succeed")
                .clone();
            let decoded_result = codec.parse(&JsonOps::INSTANCE, &encoded);
            let decoded = decoded_result.result().expect("decode should succeed");
            assert_eq!(*decoded, anchor);
        }
    }

    #[test]
    fn codec_encodes_the_dispatch_field_shape() {
        let codec = vertical_anchor_codec::<JsonOps>();
        let abs = codec
            .encode_start(&JsonOps::INSTANCE, &VerticalAnchor::absolute(5))
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(abs, json!({"absolute": 5}));
        let above = codec
            .encode_start(&JsonOps::INSTANCE, &VerticalAnchor::above_bottom(6))
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(above, json!({"above_bottom": 6}));
        let below = codec
            .encode_start(&JsonOps::INSTANCE, &VerticalAnchor::below_top(7))
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(below, json!({"below_top": 7}));
    }

    #[test]
    fn codec_rejects_out_of_range() {
        let codec = vertical_anchor_codec::<JsonOps>();
        // Below MIN_Y.
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"absolute": -2033}))
                .is_error()
        );
        // Above MAX_Y.
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"above_bottom": 2032}))
                .is_error()
        );
        // Encode validates too (the shared intRange flatXMap).
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &VerticalAnchor::below_top(2032))
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_boundary_values_are_inclusive() {
        let codec = vertical_anchor_codec::<JsonOps>();
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"absolute": MIN_Y}))
                .result()
                .is_some()
        );
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"absolute": MAX_Y}))
                .result()
                .is_some()
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &VerticalAnchor::above_bottom(MIN_Y))
                .result()
                .is_some()
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &VerticalAnchor::below_top(MAX_Y))
                .result()
                .is_some()
        );
    }

    #[test]
    fn codec_rejects_unknown_and_missing_keys() {
        let codec = vertical_anchor_codec::<JsonOps>();
        // Neither variant key present: xor tries both, both fail, the second
        // (below_top) error is surfaced.
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"not_an_anchor": 1}))
                .is_error()
        );
        // A present-but-wrong-type value also fails.
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"absolute": "x"}))
                .is_error()
        );
    }

    #[test]
    fn codec_rejects_two_matching_keys_with_xor_both_success() {
        // Java's nested xor: whenever both alternatives of an xor decode
        // successfully the XorCodec reports the exact "Both alternatives read
        // successfully" error instead of picking one — including when the
        // outer xor's first branch (absolute) and its second branch (the inner
        // above/below xor) both match.
        let codec = vertical_anchor_codec::<JsonOps>();
        for input in [
            json!({"absolute": 5, "above_bottom": 6}),
            json!({"absolute": 5, "below_top": 7}),
            json!({"above_bottom": 6, "below_top": 7}),
        ] {
            let result = codec.parse(&JsonOps::INSTANCE, &input);
            assert!(
                result.is_error(),
                "xor must reject when both alternatives read: {input}"
            );
            let msg = result
                .error_ref()
                .expect("error carries a message")
                .message()
                .to_string();
            assert!(
                msg.starts_with(
                    "Both alternatives read successfully, can not pick the correct one;"
                ),
                "unexpected xor message: {msg}"
            );
        }
        // A single matching key still decodes (the failing branch is the
        // non-matching alternative, so xor falls through to the success).
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"absolute": 5}))
                .result()
                .is_some()
        );
    }

    #[test]
    fn codec_rejects_non_map_input() {
        let codec = vertical_anchor_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!(5)).is_error());
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!([1, 2, 3]))
                .is_error()
        );
    }

    #[test]
    fn bottom_and_top_singletons() {
        assert_eq!(VerticalAnchor::bottom(), VerticalAnchor::above_bottom(0));
        assert_eq!(VerticalAnchor::top(), VerticalAnchor::below_top(0));
    }
}
