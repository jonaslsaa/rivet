//! Controlled mutation generators + expected-failure mapping for #195.
//!
//! The gate's negative control corrupts a single byte of one fixture packet,
//! which cannot detect reorderings, insertions, deletions, or normalization
//! content bugs. Each `MutationKind` below transforms a raw capture (or its
//! canonical form) and pairs with the detector(s) that must trip on it.
//! `verify --mutate <kind>` applies the mutation to a fresh raw capture (or to
//! the canonical form, for the canonical mutations) and requires the named
//! `Failure` — a clean run is itself a failure (false-negative trap).

use crate::packet::{CapturedPacket, Direction, State};

/// The kinds of corruption the harness can inject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    /// Swap two adjacent packets of different identities.
    Reorder,
    /// Drop the `accept_teleportation` (the join's required teleport ack).
    Delete,
    /// Duplicate the first `level_chunk_with_light`.
    Insert,
    /// Rewrite the `accept_teleportation` teleport id.
    Field,
    /// Corrupt the canonical form (a buggy-normalizer simulation).
    Canon,
    /// Relabel a packet's direction/state (mis-categorization).
    Relabel,
    /// Swap two mid-burst packets (the burst order the total-order rule guards).
    Burst,
    /// Corrupt an entity id in a packet the id detector covers.
    EntityId,
    /// Remove every `set_time` from the canonical form (absence detection).
    SetTimeAbsent,
}

impl MutationKind {
    pub fn all() -> Vec<MutationKind> {
        vec![
            MutationKind::Reorder,
            MutationKind::Delete,
            MutationKind::Insert,
            MutationKind::Field,
            MutationKind::Canon,
            MutationKind::Relabel,
            MutationKind::Burst,
            MutationKind::EntityId,
            MutationKind::SetTimeAbsent,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            MutationKind::Reorder => "reorder",
            MutationKind::Delete => "delete",
            MutationKind::Insert => "insert",
            MutationKind::Field => "field",
            MutationKind::Canon => "canon",
            MutationKind::Relabel => "relabel",
            MutationKind::Burst => "burst",
            MutationKind::EntityId => "entity-id",
            MutationKind::SetTimeAbsent => "set-time-absent",
        }
    }

    pub fn from_name(s: &str) -> Option<MutationKind> {
        MutationKind::all().into_iter().find(|k| k.name() == s)
    }

    /// Detector kinds the mutation must trip (observed on the real raw capture,
    /// `each_mutation_trips_its_expected_detector`). At least one `Failure` whose
    /// kind is listed here must be produced after the mutation; an empty
    /// intersection means the mutation slipped through undetected.
    pub fn expected_kinds(self) -> &'static [&'static str] {
        match self {
            MutationKind::Reorder => &["ordering"],
            MutationKind::Delete => &["teleport-ack"],
            MutationKind::Insert => &["chunk"],
            MutationKind::Field => &["teleport-ack"],
            MutationKind::Canon => &["set_time"],
            MutationKind::Relabel => &["ordering", "chunk"],
            MutationKind::Burst => &["ordering"],
            MutationKind::EntityId => &["entity-id"],
            MutationKind::SetTimeAbsent => &["set_time"],
        }
    }
}

/// Apply a mutation to a raw capture. Deterministic (seeded by packet content),
/// so `verify --mutate` reproduces the exact failure across runs.
pub fn mutate_raw(kind: MutationKind, raw: &[CapturedPacket]) -> Vec<CapturedPacket> {
    let mut out = raw.to_vec();
    match kind {
        MutationKind::Reorder => {
            // Swap two adjacent packets of different identities.
            if let Some(i) = out.windows(2).position(|w| {
                !(w[0].state == w[1].state
                    && w[0].direction == w[1].direction
                    && w[0].id == w[1].id)
            }) {
                out.swap(i, i + 1);
            }
        }
        MutationKind::Delete => {
            // Drop the accept_teleportation (a required join packet).
            if let Some(i) = out.iter().position(|p| {
                p.state == State::Play && p.direction == Direction::Serverbound && p.id == 0
            }) {
                out.remove(i);
            }
        }
        MutationKind::Insert => {
            // Duplicate the first chunk (breaks the chunk-grid shape + ordering).
            if let Some(i) = out.iter().position(|p| {
                p.state == State::Play && p.direction == Direction::Clientbound && p.id == 45
            }) {
                let dup = out[i].clone();
                out.insert(i + 1, dup);
            }
        }
        MutationKind::Field => {
            // Perturb the accept_teleportation teleport id.
            if let Some(p) = out.iter_mut().find(|p| {
                p.state == State::Play && p.direction == Direction::Serverbound && p.id == 0
            }) {
                p.body = vec![0x7F];
            }
        }
        MutationKind::Canon => {
            // No raw mutation: `verify --mutate canon` corrupts the canonical
            // form instead (see `mutate_canon`).
        }
        MutationKind::Relabel => {
            // Relabel the first chunk as serverbound (mis-categorization).
            if let Some(p) = out.iter_mut().find(|p| {
                p.state == State::Play && p.direction == Direction::Clientbound && p.id == 45
            }) {
                p.direction = Direction::Serverbound;
            }
        }
        MutationKind::Burst => {
            // Swap two packets inside the deterministic play burst. The burst is
            // the join's fixed send order (see ordering.rs PLAY_BURST_ORDER); a
            // reorder here still byte-matches the id-grouped canonical fixture,
            // so only the ordering detector catches it. Pick the first adjacent
            // pair of different burst packet identities.
            use crate::ordering::PLAY_BURST_ORDER;
            if let Some(i) = out.windows(2).position(|w| {
                w[0].state == State::Play
                    && w[0].direction == Direction::Clientbound
                    && PLAY_BURST_ORDER.contains(&w[0].id)
                    && w[1].state == State::Play
                    && w[1].direction == Direction::Clientbound
                    && PLAY_BURST_ORDER.contains(&w[1].id)
                    && w[0].id != w[1].id
            }) {
                out.swap(i, i + 1);
            }
        }
        MutationKind::EntityId => {
            // Corrupt the update_attributes entity id (a VarInt head the
            // entity-id detector must name). update_attributes is the only
            // entity-id packet present in every join capture.
            if let Some(p) = out.iter_mut().find(|p| {
                p.state == State::Play && p.direction == Direction::Clientbound && p.id == 131
            }) {
                p.body = vec![42];
            }
        }
        MutationKind::SetTimeAbsent => {
            // No raw mutation: `verify --mutate set-time-absent` drops the
            // canonical set_time instead (see `mutate_set_time_absent`).
        }
    }
    out
}

/// Apply a canonical-form corruption (a buggy-normalizer simulation): truncate
/// the first `set_time` body, which the set_time structural detector must name.
pub fn mutate_canon(canon: &mut [crate::normalize::NormalizedPacket]) {
    if let Some(p) = canon
        .iter_mut()
        .find(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == 113)
    {
        // Truncate mid-body: gameTime + a count that no longer matches the
        // remaining entries.
        p.body.truncate(9);
    }
}

/// Apply a canonical-form corruption: drop every `set_time` packet. This
/// simulates a normalizer that drops the world-clock sync; the set_time
/// absence detector (semantic::check_set_time) must name it.
pub fn mutate_set_time_absent(canon: &mut Vec<crate::normalize::NormalizedPacket>) {
    canon.retain(|p| {
        !(p.state == State::Play && p.direction == Direction::Clientbound && p.id == 113)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::write_varint;
    use crate::normalize::NormalizedPacket;

    fn pkt(state: State, direction: Direction, id: i32, body: Vec<u8>) -> CapturedPacket {
        CapturedPacket {
            state,
            direction,
            id,
            body,
        }
    }

    fn sample_raw() -> Vec<CapturedPacket> {
        let mut b = Vec::new();
        write_varint(&mut b, 776);
        write_varint(&mut b, 9);
        b.extend_from_slice(b"127.0.0.1");
        b.extend_from_slice(&25598u16.to_be_bytes());
        write_varint(&mut b, 2);
        vec![
            pkt(State::Handshake, Direction::Serverbound, 0, b),
            pkt(State::Login, Direction::Serverbound, 0, vec![0x00]),
            pkt(
                State::Play,
                Direction::Clientbound,
                49,
                1i32.to_be_bytes().to_vec(),
            ),
            pkt(State::Play, Direction::Clientbound, 45, vec![0x00; 16]),
            pkt(State::Play, Direction::Serverbound, 0, vec![0x01]),
        ]
    }

    #[test]
    fn reorder_swaps_adjacent_distinct() {
        let m = mutate_raw(MutationKind::Reorder, &sample_raw());
        // The first distinct adjacent pair is login_hello + login (login state
        // vs play state) — swapped.
        assert_ne!(m, sample_raw());
    }

    #[test]
    fn delete_removes_accept_teleportation() {
        let m = mutate_raw(MutationKind::Delete, &sample_raw());
        assert!(
            !m.iter().any(|p| p.state == State::Play
                && p.direction == Direction::Serverbound
                && p.id == 0)
        );
    }

    #[test]
    fn insert_duplicates_first_chunk() {
        let m = mutate_raw(MutationKind::Insert, &sample_raw());
        let chunks = m
            .iter()
            .filter(|p| {
                p.state == State::Play && p.direction == Direction::Clientbound && p.id == 45
            })
            .count();
        assert_eq!(chunks, 2);
    }

    #[test]
    fn field_changes_ack_id() {
        let m = mutate_raw(MutationKind::Field, &sample_raw());
        let ack = m
            .iter()
            .find(|p| p.state == State::Play && p.direction == Direction::Serverbound && p.id == 0)
            .expect("ack");
        assert_eq!(ack.body, vec![0x7F]);
    }

    #[test]
    fn canon_truncates_set_time() {
        let mut canon = vec![NormalizedPacket {
            state: State::Play,
            direction: Direction::Clientbound,
            id: 113,
            body: vec![0x00; 29],
            note: String::new(),
        }];
        mutate_canon(&mut canon);
        assert_eq!(canon[0].body.len(), 9);
    }

    #[test]
    fn burst_swaps_mid_burst_pair() {
        let m = mutate_raw(MutationKind::Burst, &sample_raw());
        // sample_raw has no burst packets; the mutation must be a no-op there.
        assert_eq!(m, sample_raw());
    }

    #[test]
    fn burst_mutation_still_trips_on_a_fixed_burst_layout() {
        // With commands (16) no longer in PLAY_BURST_ORDER, the Burst mutation
        // must still find an adjacent fixed-burst pair on a real join layout and
        // swap it into an ordering violation — `each_mutation_trips_its_expected_detector`
        // stays non-vacuous for the fixed packets.
        let mut raw = vec![pkt(State::Handshake, Direction::Serverbound, 0, vec![0x00])];
        for id in crate::ordering::PLAY_BURST_ORDER {
            raw.push(pkt(State::Play, Direction::Clientbound, *id, vec![]));
        }
        let mutated = mutate_raw(MutationKind::Burst, &raw);
        assert_ne!(
            mutated, raw,
            "Burst must swap a pair of fixed burst packets"
        );
        let failures = crate::ordering::check(&mutated);
        assert!(
            failures.iter().any(|f| f.kind == "ordering"),
            "Burst mutation must produce an ordering failure, got {failures:?}"
        );
    }

    #[test]
    fn entity_id_corrupts_update_attributes() {
        let m = mutate_raw(MutationKind::EntityId, &sample_raw());
        let p = m.iter().find(|p| {
            p.state == State::Play && p.direction == Direction::Clientbound && p.id == 131
        });
        assert!(p.is_none()); // sample_raw has no update_attributes — no-op.
    }

    #[test]
    fn set_time_absent_removes_all() {
        let mut canon = vec![NormalizedPacket {
            state: State::Play,
            direction: Direction::Clientbound,
            id: 113,
            body: vec![0x00; 9],
            note: String::new(),
        }];
        mutate_set_time_absent(&mut canon);
        assert!(canon.is_empty());
    }
}
