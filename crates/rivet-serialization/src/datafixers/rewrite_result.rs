//! Port of `com.mojang.datafixers.RewriteResult`.
//!
//! Java `RewriteResult<A, B>` is a record of a `View<A, B>` and a `BitSet`
//! `recData` (the set of recursive-type indices the result's function
//! touches). The port keeps a plain `Vec<usize>` for the bitset and erases the
//! type parameters.

use crate::datafixers::types::Type;
use crate::datafixers::view::View;
use crate::dynamic_ops::DynamicOps;
use std::fmt::Debug;

/// `com.mojang.datafixers.RewriteResult<A, B>` (type-erased).
pub struct RewriteResult<Ops: DynamicOps + 'static> {
    pub view: View<Ops>,
    /// `BitSet` of recursive-type indices touched.
    pub rec_data: Vec<usize>,
}

impl<Ops: DynamicOps + 'static> Clone for RewriteResult<Ops> {
    fn clone(&self) -> Self {
        RewriteResult {
            view: self.view.clone(),
            rec_data: self.rec_data.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for RewriteResult<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RR[{:?}]", self.view)
    }
}

impl<Ops: DynamicOps + 'static> RewriteResult<Ops> {
    /// `RewriteResult.create(view, recData)`.
    pub fn create(view: View<Ops>, rec_data: Vec<usize>) -> Self {
        RewriteResult { view, rec_data }
    }

    /// `RewriteResult.nop(Type<A>)` — the identity view and empty bitset.
    pub fn nop(ty: &dyn Type<Ops>) -> Self {
        RewriteResult {
            view: View::nop_view(ty.clone_ty()),
            rec_data: Vec::new(),
        }
    }

    /// `RewriteResult.compose(RewriteResult)`.
    pub fn compose(&self, that: &RewriteResult<Ops>) -> RewriteResult<Ops> {
        let new_data =
            if self.view.input.is_recursive_point() && that.view.input.is_recursive_point() {
                // same family, merge results - not exactly accurate, but should be good enough
                let mut merged = self.rec_data.clone();
                for &i in &that.rec_data {
                    if !merged.contains(&i) {
                        merged.push(i);
                    }
                }
                merged
            } else {
                self.rec_data.clone()
            };
        RewriteResult {
            view: self.view.compose(&that.view),
            rec_data: new_data,
        }
    }

    /// Convenience accessor for the view.
    pub fn view(&self) -> &View<Ops> {
        &self.view
    }
}

/// `RewriteResult.create(View, BitSet)` helper for the `View`-only form used by
/// `RecursiveTypeFamily.everywhere`.
pub fn create_view<Ops: DynamicOps + 'static>(
    view: View<Ops>,
    rec_data: Vec<usize>,
) -> RewriteResult<Ops> {
    RewriteResult::create(view, rec_data)
}
