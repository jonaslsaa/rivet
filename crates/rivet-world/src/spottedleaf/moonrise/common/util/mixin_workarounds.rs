//! `ca.spottedleaf.moonrise.common.util.MixinWorkarounds` — `long[]`/`byte[]`
//! `clone()` (a Mixin workaround in Java; a plain copy here).
//!
//! Java source:
//! `working/Paper/paper-server/src/main/java/ca/spottedleaf/moonrise/common/util/MixinWorkarounds.java`.

/// `MixinWorkarounds.clone(long[])`.
pub fn clone(values: &[u64]) -> Vec<u64> {
    values.to_vec()
}

/// `MixinWorkarounds.clone(byte[])`.
pub fn clone_bytes(values: &[u8]) -> Vec<u8> {
    values.to_vec()
}
