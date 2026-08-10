//! Port of `net.minecraft.util.KeyDispatchDataCodec`.
//!
//! A plain value wrapper over a `MapCodec<A>` — the per-type codec record
//! carried by `DensityFunction`/`BlockState`/surface-rule implementations. It
//! adds no behavior beyond holding the `MapCodec` (Paper's `DensityFunction`
//! returns it from `codec()` and the dispatch layer reads `KeyDispatchCodec`
//! out of it), so the port is a transparent newtype around `Arc<dyn MapCodec>`.

use crate::map_codec::MapCodec;
use std::fmt::Debug;
use std::sync::Arc;

/// `net.minecraft.util.KeyDispatchDataCodec<A>`.
#[derive(Clone)]
pub struct KeyDispatchDataCodec<A, Ops: crate::dynamic_ops::DynamicOps + 'static> {
    codec: Arc<dyn MapCodec<A, Ops>>,
}

impl<A, Ops: crate::dynamic_ops::DynamicOps + 'static> Debug for KeyDispatchDataCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyDispatchDataCodec[{:?}]", self.codec)
    }
}

impl<A, Ops: crate::dynamic_ops::DynamicOps + 'static> KeyDispatchDataCodec<A, Ops> {
    /// `KeyDispatchDataCodec.of(MapCodec<A>)`.
    pub fn of(codec: Arc<dyn MapCodec<A, Ops>>) -> Self
    where
        A: 'static,
    {
        KeyDispatchDataCodec { codec }
    }

    /// The wrapped `MapCodec<A>` — Java record accessor `codec()`.
    pub fn codec(&self) -> Arc<dyn MapCodec<A, Ops>>
    where
        A: 'static,
    {
        self.codec.clone()
    }
}
