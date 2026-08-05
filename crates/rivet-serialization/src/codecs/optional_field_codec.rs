//! Port of `com.mojang.serialization.codecs.OptionalFieldCodec`.
//!
//! "Optimization of `Codec.either(someCodec.field(name), Codec.EMPTY)`".

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use crate::map_codec::MapCodec;
use std::fmt::Debug;
use std::sync::Arc;

/// `OptionalFieldCodec<A>`.
pub struct OptionalFieldCodec<A, Ops: DynamicOps + 'static> {
    pub name: String,
    pub element_codec: Arc<dyn Codec<A, Ops>>,
    pub lenient: bool,
}

impl<A, Ops: DynamicOps + 'static> OptionalFieldCodec<A, Ops> {
    fn key(&self, ops: &Ops) -> Ops::Output {
        ops.create_string(self.name.clone())
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for OptionalFieldCodec<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![ops.create_string(self.name.clone())]
    }
}

impl<A, Ops: DynamicOps + 'static> MapCodec<Option<A>, Ops> for OptionalFieldCodec<A, Ops>
where
    A: Clone,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<Option<A>> {
        let value = input.get_string(&self.name);
        let value = match value {
            Some(v) => v,
            None => return DataResult::success(None),
        };
        let parsed = self.element_codec.parse(ops, &value);
        if parsed.is_error() && self.lenient {
            return DataResult::success(None);
        }
        // `parsed.map(Optional::of).setPartial(parsed.resultOrPartial())`
        parsed
            .clone()
            .map(|a| Some(a.clone()))
            .set_partial(parsed.result_or_partial_silent())
    }

    fn encode(
        &self,
        input: &Option<A>,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        match input {
            Some(value) => {
                prefix.add_string_result(&self.name, self.element_codec.encode_start(ops, value));
            }
            None => {}
        }
    }
}

impl<A, Ops: DynamicOps + 'static> Debug for OptionalFieldCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OptionalFieldCodec[{}: {:?}]",
            self.name, self.element_codec
        )
    }
}
