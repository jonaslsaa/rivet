//! `net.minecraft.world.phys.shapes.BooleanOp` — the functional interface over
//! two booleans and its sixteen constants.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/BooleanOp.java`.

/// `BooleanOp` — `boolean apply(boolean first, boolean second)`.
#[derive(Clone, Copy)]
pub struct BooleanOp {
    apply: fn(bool, bool) -> bool,
}

impl std::fmt::Debug for BooleanOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BooleanOp")
    }
}

impl BooleanOp {
    /// A lambda `(first, second) -> ...`.
    pub const fn new(apply: fn(bool, bool) -> bool) -> BooleanOp {
        BooleanOp { apply }
    }

    /// `BooleanOp.apply(boolean, boolean)`.
    pub fn apply(&self, first: bool, second: bool) -> bool {
        (self.apply)(first, second)
    }

    /// `BooleanOp.FALSE`.
    pub const FALSE: BooleanOp = BooleanOp::new(|_first, _second| false);
    /// `BooleanOp.NOT_OR`.
    pub const NOT_OR: BooleanOp = BooleanOp::new(|first, second| !first && !second);
    /// `BooleanOp.ONLY_SECOND`.
    pub const ONLY_SECOND: BooleanOp = BooleanOp::new(|first, second| second && !first);
    /// `BooleanOp.NOT_FIRST`.
    pub const NOT_FIRST: BooleanOp = BooleanOp::new(|first, _second| !first);
    /// `BooleanOp.ONLY_FIRST`.
    pub const ONLY_FIRST: BooleanOp = BooleanOp::new(|first, second| first && !second);
    /// `BooleanOp.NOT_SECOND`.
    pub const NOT_SECOND: BooleanOp = BooleanOp::new(|_first, second| !second);
    /// `BooleanOp.NOT_SAME`.
    pub const NOT_SAME: BooleanOp = BooleanOp::new(|first, second| first != second);
    /// `BooleanOp.NOT_AND`.
    pub const NOT_AND: BooleanOp = BooleanOp::new(|first, second| !first || !second);
    /// `BooleanOp.AND`.
    pub const AND: BooleanOp = BooleanOp::new(|first, second| first && second);
    /// `BooleanOp.SAME`.
    pub const SAME: BooleanOp = BooleanOp::new(|first, second| first == second);
    /// `BooleanOp.SECOND`.
    pub const SECOND: BooleanOp = BooleanOp::new(|_first, second| second);
    /// `BooleanOp.CAUSES`.
    pub const CAUSES: BooleanOp = BooleanOp::new(|first, second| !first || second);
    /// `BooleanOp.FIRST`.
    pub const FIRST: BooleanOp = BooleanOp::new(|first, _second| first);
    /// `BooleanOp.CAUSED_BY`.
    pub const CAUSED_BY: BooleanOp = BooleanOp::new(|first, second| first || !second);
    /// `BooleanOp.OR`.
    pub const OR: BooleanOp = BooleanOp::new(|first, second| first || second);
    /// `BooleanOp.TRUE`.
    pub const TRUE: BooleanOp = BooleanOp::new(|_first, _second| true);
}
