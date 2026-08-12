//! Port of `com.mojang.datafixers.Typed`.
//!
//! Java `Typed<A>` is a `Type<A>` plus the `DynamicOps` and a value `A`. The
//! port erases the value to [`AnyValue`] and holds the ops in the type
//! parameter (`Typed<Ops>`), so the ops field is dropped (Java's `Typed.ops`
//! only re-appears at the optics layer, which is deferred).

use crate::data_result::DataResult;
use crate::datafixers::types::{AnyValue, Type};
use crate::dynamic::Dynamic;
use crate::dynamic_ops::DynamicOps;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.datafixers.Typed<A>` (value erased to [`AnyValue`]).
pub struct Typed<Ops: DynamicOps + 'static> {
    pub ty: Arc<dyn Type<Ops>>,
    pub value: AnyValue,
}

impl<Ops: DynamicOps + 'static> Debug for Typed<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Typed[?]")
    }
}

impl<Ops: DynamicOps + 'static> Clone for Typed<Ops> {
    fn clone(&self) -> Self {
        Typed {
            ty: self.ty.clone(),
            value: self.value.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Typed<Ops> {
    /// `new Typed<>(type, ops, value)` — ops is implicit in `Ops`.
    pub fn new(ty: Arc<dyn Type<Ops>>, value: AnyValue) -> Self {
        Typed { ty, value }
    }

    /// `Typed.getType()`.
    pub fn ty(&self) -> Arc<dyn Type<Ops>> {
        self.ty.clone()
    }

    /// `Typed.getValue()`.
    pub fn value(&self) -> &AnyValue {
        &self.value
    }

    /// `Typed.write()` — `type.writeDynamic(ops, value)`.
    pub fn write(&self, ops: &Ops) -> DataResult<Dynamic<Ops::Output>> {
        self.ty.write_dynamic(ops, &self.value)
    }

    /// `Typed.toString()`; the value is erased, so the type is printed instead.
    pub fn to_debug_string(&self) -> String {
        format!("Typed[{}]", self.ty.type_to_string())
    }
}
