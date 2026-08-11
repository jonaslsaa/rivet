//! Port of `net.minecraft.world.level.levelgen.blockpredicates.CombiningPredicate`
//! (abstract class, 26.2).
//!
//! Java is the abstract base of `AllOfPredicate`/`AnyOfPredicate`: it holds the
//! `protected final List<BlockPredicate> predicates` and exposes the shared
//! `codec(Function<List<BlockPredicate>, T>)` record codec over the
//! `"predicates"` field (`BlockPredicate.CODEC.listOf()`). The Rust port is a
//! standalone trait whose `predicates()` accessor the concrete combining
//! predicates implement, plus the shared ops-generic codec constructor.

use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt::Debug;
use std::sync::Arc;

/// `Function<List<BlockPredicate>, T>` — the `CombiningPredicate.codec` ctor.
type PredicatesCtor<P> = Arc<dyn Fn(Vec<Arc<dyn BlockPredicate>>) -> P + Send + Sync>;

/// `net.minecraft.world.level.levelgen.blockpredicates.CombiningPredicate` —
/// the base of predicates that combine a list of child predicates.
///
/// Concrete combinators implement this (the Rust analogue of extending the
/// abstract class) and provide the `predicates` list; the shared record codec
/// reads it via the `"predicates"` field.
pub trait CombiningPredicate: Debug + Send + Sync + 'static {
    /// `this.predicates` — the child predicate list.
    fn predicates(&self) -> &[Arc<dyn BlockPredicate>];
}

/// `CombiningPredicate.codec(Function<List<BlockPredicate>, T>)` — the shared
/// record codec over `"predicates"` (`BlockPredicate.CODEC.listOf()`), as the
/// ops-generic `combining_predicate_codec::<T, Ops>()` factory.
///
/// `top` is the `RecursiveSelf` of the block-predicate graph (the same shared
/// `BlockPredicate.CODEC` the child list recurses into — see
/// `block_predicate::block_predicate_codec`).
pub fn combining_predicate_codec<P, Ops>(
    constructor: PredicatesCtor<P>,
    top: Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>>,
) -> Arc<dyn MapCodec<P, Ops>>
where
    P: CombiningPredicate + 'static,
    Ops: DynamicOps + 'static,
{
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|p: &P| p.predicates().to_vec()),
                "predicates".to_string(),
                codec::list(top),
            ))
            .apply(instance, constructor)
    })
}
