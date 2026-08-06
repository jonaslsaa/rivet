//! Port of `net.minecraft.network.chat.contents.ScoreContents`.
//!
//! Holds a `name` (an `Either<CompilableString<EntitySelector>, String>` — the
//! selector variant is deferred, so this slice models the plain-string
//! `Either.right` form and a deferred `Selector` placeholder) and an
//! `objective`. Resolution (scoreboard lookup) is out of scope.

use super::super::ComponentContents;
use crate::style::Style;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::map_codec::{self as map_codec_mod, MapCodec};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::sync::Arc;

/// `ScoreContents.name` — `Either<CompilableString<EntitySelector>, String>`.
/// The `CompilableString<EntitySelector>` variant is deferred (brigadier +
/// entity selectors); this slice models the plain-string form, matching the
/// only path `ComponentSerialization.CODEC` produces when it can't compile a
/// selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScoreName {
    /// `Either.right(String)` — a plain player name.
    Name(String),
    /// Deferred `Either.left(CompilableString<EntitySelector>)`.
    Selector(String),
}

/// Port of `net.minecraft.network.chat.contents.ScoreContents`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreContents {
    name: ScoreName,
    objective: String,
}

impl ScoreContents {
    /// `new ScoreContents(name, objective)`.
    pub fn new(name: ScoreName, objective: String) -> Self {
        ScoreContents { name, objective }
    }

    /// `ScoreContents.name()`.
    pub fn name(&self) -> &ScoreName {
        &self.name
    }

    /// `ScoreContents.objective()`.
    pub fn objective(&self) -> &str {
        &self.objective
    }

    /// `visit(ContentConsumer)` — scoreboard resolution is out of scope.
    pub fn visit_content<T>(&self, _output: &mut dyn FnMut(&str) -> Option<T>) -> Option<T> {
        None
    }

    /// `visit(StyledContentConsumer, Style)` — same deferral.
    pub fn visit_styled<T>(
        &self,
        _output: &mut dyn FnMut(&Style, &str) -> Option<T>,
        _style: &Style,
    ) -> Option<T> {
        None
    }

    /// `ScoreContents.MAP_CODEC` — `INNER_CODEC.fieldOf("score")`, lifted to
    /// the `ComponentContents` enum.
    pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ComponentContents, Ops>> {
        let inner: Arc<dyn MapCodec<ScoreContents, Ops>> = Arc::new(ScoreInnerCodec {
            _ops: std::marker::PhantomData,
        });
        // `INNER_CODEC.fieldOf("score")` — lift the inner MapCodec to a Codec
        // then apply `Codec.fieldOf`.
        let field: Arc<dyn MapCodec<ScoreContents, Ops>> = rivet_serialization::codec::field_of(
            map_codec_mod::codec_of(inner),
            "score".to_string(),
        );
        map_codec_mod::xmap(
            field,
            Arc::new(|c: &ScoreContents| ComponentContents::Score(c.clone())),
            Arc::new(|c: &ComponentContents| match c {
                ComponentContents::Score(inner) => inner.clone(),
                _ => panic!("score codec applied to non-score contents"),
            }),
        )
    }
}

/// `ScoreContents.INNER_CODEC` — the `{name, objective}` record shape, before
/// the `fieldOf("score")` wrap.
struct ScoreInnerCodec<Ops: DynamicOps + 'static> {
    _ops: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for ScoreInnerCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ScoreContentsInnerCodec")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for ScoreInnerCodec<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![
            ops.create_string("name".to_string()),
            ops.create_string("objective".to_string()),
        ]
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<ScoreContents, Ops> for ScoreInnerCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<ScoreContents> {
        let (Some(name), Some(objective)) =
            (input.get_string("name"), input.get_string("objective"))
        else {
            return DataResult::error("No key name/objective in MapLike".to_string());
        };
        let name = codec::string_codec::<Ops>().parse(ops, &name);
        let objective = codec::string_codec::<Ops>().parse(ops, &objective);
        name.flat_map(move |name| {
            objective.map(move |objective| {
                // Java's `Codec.either(EntitySelector.COMPILABLE_CODEC, STRING)`
                // always yields `Either.right` here because the selector is
                // never compiled.
                ScoreContents::new(ScoreName::Name(name.clone()), objective.clone())
            })
        })
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<ScoreContents, Ops> for ScoreInnerCodec<Ops> {
    fn encode(
        &self,
        input: &ScoreContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        let name = match &input.name {
            ScoreName::Name(n) => n,
            ScoreName::Selector(s) => s,
        };
        prefix.add_string_result("name", codec::string_codec::<Ops>().encode_start(ops, name));
        prefix.add_string_result(
            "objective",
            codec::string_codec::<Ops>().encode_start(ops, &input.objective),
        );
    }
}

impl<Ops: DynamicOps + 'static> MapCodec<ScoreContents, Ops> for ScoreInnerCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<ScoreContents> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &ScoreContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}

impl std::fmt::Display for ScoreContents {
    /// `ScoreContents.toString()` = `score{name='N', objective='O'}` where `N`
    /// is the `name` `Either`'s `toString` — `Right[value]` / `Left[value]`,
    /// and `CompilableString.toString` is its raw source.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("score{name='")?;
        match &self.name {
            ScoreName::Name(n) => write!(f, "Right[{n}]")?,
            ScoreName::Selector(s) => write!(f, "Left[{s}]")?,
        }
        write!(f, "', objective='{}'}}", self.objective)
    }
}
