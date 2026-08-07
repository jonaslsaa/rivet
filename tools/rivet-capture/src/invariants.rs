//! The detector runner for #195: every independent semantic/order check over a
//! raw capture and its canonical form, aggregated into a single `Vec<Failure>`.
//!
//! Each detector is a read-only parser built only on `frame.rs` leaf primitives
//! and the generated packet tables — it never calls `normalize.rs` or
//! `structured.rs` re-serializers. This is the independence principle that
//! defeats a self-consistent normalizer: if `canonicalize` dropped, duplicated,
//! or altered content, the raw↔canonical preservation checks in `semantic.rs`
//! diverge even when the committed fixture was produced by the same (buggy)
//! normalizer.
//!
//! Detector classes:
//!
//! - `ordering.rs` — within-direction + response-chain ordering of the raw
//!   capture (the join state machine's required packet order), including the
//!   deterministic play burst total order.
//! - `relationships.rs` — id-matched and coordinate relationships on the raw
//!   capture (teleport→ack, keepalive request→echo, spawn/movement
//!   consistency, entity ids across every entity-id packet).
//! - `semantic.rs` — read-only wire parsers for chunks, registry_data,
//!   update_tags, and set_time (including set_time absence), plus the
//!   raw↔canonical content-preservation equalities and the fixture's
//!   world-shape invariants.

use std::fmt;

use crate::normalize::NormalizedPacket;
use crate::packet::CapturedPacket;
use crate::{ordering, relationships, semantic};

/// One named detector failure. `kind` is a stable slug (the detector family),
/// `identity` names the offending packet or field, and `message` explains the
/// violation. The mutation tests match on `kind` + an identity substring, so
/// both must be deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub kind: &'static str,
    pub identity: String,
    pub message: String,
}

impl Failure {
    pub fn new(
        kind: &'static str,
        identity: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            identity: identity.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.kind, self.identity, self.message)
    }
}

/// Run every detector over a raw capture and its canonical form. Returns all
/// violations (empty when the capture satisfies every invariant).
pub fn check(raw: &[CapturedPacket], canon: &[NormalizedPacket]) -> Vec<Failure> {
    let mut out = Vec::new();
    // Raw ground-truth checks (ordering + id/coordinate relationships).
    out.extend(ordering::check(raw));
    out.extend(relationships::check(raw));
    // Raw semantic checks (world shape, registry id-range/coverage).
    out.extend(semantic::check_chunk_semantics(raw));
    out.extend(semantic::check_registry_tags(raw));
    // Canonical structural checks + raw↔canonical content preservation.
    out.extend(semantic::check_set_time(canon));
    out.extend(semantic::check_preservation(raw, canon));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_display_names_kind_identity_message() {
        let f = Failure::new(
            "ordering",
            "play/clientbound player_position",
            "precedes the ack",
        );
        assert_eq!(
            f.to_string(),
            "ordering: play/clientbound player_position: precedes the ack"
        );
    }
}
