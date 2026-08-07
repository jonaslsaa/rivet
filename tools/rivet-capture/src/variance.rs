//! Multi-boot raw-field variance audit for #195.
//!
//! The fixture's normalization claims a specific set of wire fields vary per
//! boot (keepalive ids, sessionId, set_time, per-boot HashMap orders, racy
//! counts). `audit` boots `--runs N` Papers and reports, per packet identity,
//! how many distinct raw bodies were observed. Fields with `distinct == 1`
//! across N boots are either truly deterministic (in which case their
//! normalization is masking a real value) or the run count is too small —
//! the audit names them so the evidence is explicit rather than assumed.

use std::collections::{BTreeMap, BTreeSet};

use crate::packet::{CapturedPacket, Direction, State};

/// Per (state, direction, id): the set of distinct raw body SHA-256s observed.
pub struct VarianceReport {
    /// identity -> (distinct count, body hexes).
    pub fields: Vec<(String, usize, Vec<String>)>,
}

/// Summarize a multi-boot raw capture set.
pub fn analyze(runs: &[Vec<CapturedPacket>]) -> VarianceReport {
    let mut per: BTreeMap<(State, Direction, i32), BTreeSet<Vec<u8>>> = BTreeMap::new();
    for raw in runs {
        for p in raw {
            per.entry((p.state, p.direction, p.id))
                .or_default()
                .insert(p.body.clone());
        }
    }
    let mut fields = Vec::new();
    for ((state, dir, id), bodies) in per {
        let distinct = bodies.len();
        let mut hexes: Vec<String> = bodies
            .into_iter()
            .map(|b| crate::fixture::hex(&b))
            .collect();
        hexes.sort();
        let identity = crate::ordering::identity(state, dir, id);
        fields.push((identity, distinct, hexes));
    }
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    VarianceReport { fields }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_distinct_bodies_per_identity() {
        let boot1 = vec![CapturedPacket {
            state: State::Play,
            direction: Direction::Clientbound,
            id: 44,
            body: vec![1, 2, 3],
        }];
        let boot2 = vec![CapturedPacket {
            state: State::Play,
            direction: Direction::Clientbound,
            id: 44,
            body: vec![4, 5, 6],
        }];
        let boot3 = vec![CapturedPacket {
            state: State::Play,
            direction: Direction::Clientbound,
            id: 44,
            body: vec![1, 2, 3],
        }];
        let report = analyze(&[boot1, boot2, boot3]);
        assert_eq!(report.fields.len(), 1);
        assert_eq!(report.fields[0].1, 2);
    }
}
