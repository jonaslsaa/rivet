//! Moonrise NewChunkHolder's FULL-neighbour and ticket readiness calculation.
//! Completed generation is distinct from CURRENT border/ticking status: only
//! the host executing the actual status-change effects may publish CURRENT.
use super::{
    chunk_level,
    full_chunk_status::{FullChunkStatus, FullChunkStatusBinding},
};

const fn loaded_mask(radius: i32) -> u64 {
    let mut mask = 0;
    let mut x = -radius;
    while x <= radius {
        let mut z = -radius;
        while z <= radius {
            mask |= 1 << (x + z * 5 + 12);
            z += 1;
        }
        x += 1;
    }
    mask
}

pub(super) struct HolderReadiness {
    ticket_level: i32,
    generation_full: bool,
    full_neighbours: u64,
    border_published: bool,
    pending: FullChunkStatus,
    current: FullChunkStatusBinding,
    processing: bool,
}
impl Default for HolderReadiness {
    fn default() -> Self {
        Self {
            ticket_level: i32::MAX,
            generation_full: false,
            full_neighbours: 0,
            border_published: false,
            pending: FullChunkStatus::Inaccessible,
            current: FullChunkStatusBinding::new(FullChunkStatus::Inaccessible),
            processing: false,
        }
    }
}
impl HolderReadiness {
    pub(super) fn binding(&self) -> FullChunkStatusBinding {
        self.current.clone()
    }
    pub(super) fn current(&self) -> FullChunkStatus {
        self.current.get()
    }
    pub(super) fn pending(&self) -> FullChunkStatus {
        self.pending
    }
    pub(super) fn border_published(&self) -> bool {
        self.border_published
    }
    pub(super) fn set_border_published(&mut self, loaded: bool) {
        self.border_published = loaded;
    }
    pub(super) fn set_ticket_level(&mut self, level: i32) {
        self.ticket_level = level;
        self.update_pending();
    }
    pub(super) fn set_generation_full(&mut self) {
        self.generation_full = true;
        self.update_pending();
    }
    pub(super) fn set_neighbour(&mut self, x: i32, z: i32, loaded: bool) {
        assert!(
            (-2..=2).contains(&x) && (-2..=2).contains(&z),
            "source FULL neighbour cache radius"
        );
        let bit = 1_u64 << (x + z * 5 + 12);
        if loaded {
            self.full_neighbours |= bit;
        } else {
            self.full_neighbours &= !bit;
        }
        self.update_pending();
    }
    fn update_pending(&mut self) {
        let by_ticket = chunk_level::full_status(self.ticket_level);
        let mut pending = if self.full_neighbours & loaded_mask(2) == loaded_mask(2) {
            FullChunkStatus::EntityTicking
        } else if self.full_neighbours & loaded_mask(1) == loaded_mask(1) {
            FullChunkStatus::BlockTicking
        } else if self.full_neighbours & loaded_mask(0) == loaded_mask(0) {
            FullChunkStatus::Full
        } else {
            FullChunkStatus::Inaccessible
        };
        if pending == FullChunkStatus::Inaccessible
            && by_ticket.is_or_after(FullChunkStatus::Full)
            && self.generation_full
        {
            pending = FullChunkStatus::Full;
        }
        if pending.is_or_after(by_ticket) {
            pending = by_ticket;
        }
        self.pending = pending;
    }
    /// One source ladder step. Upgrade publication precedes callbacks; downgrade
    /// publication follows them. The host must preserve that ordering itself.
    pub(super) fn next(&self) -> Option<FullChunkStatus> {
        let current = self.current();
        if current == self.pending {
            None
        } else if self.pending.is_or_after(current) {
            Some(FullChunkStatus::ALL[current as usize + 1])
        } else {
            Some(FullChunkStatus::ALL[current as usize - 1])
        }
    }
    pub(super) fn publish_current(&mut self, status: FullChunkStatus) -> bool {
        if (self.current() as i32 - status as i32).abs() != 1 {
            return false;
        }
        self.current.set(status);
        true
    }
    pub(super) fn begin_processing(&mut self) -> bool {
        if self.processing {
            false
        } else {
            self.processing = true;
            true
        }
    }
    pub(super) fn end_processing(&mut self) {
        self.processing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_paper_ticket_and_full_neighbour_readiness() {
        let mut count = 0;
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/holder-readiness.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let mut holder = HolderReadiness::default();
            holder.set_ticket_level(row["ticket"].as_i64().unwrap() as i32);
            if row["generation_full"].as_bool().unwrap() {
                holder.set_generation_full();
            }
            let bits = row["bits"].as_u64().unwrap();
            for z in -2..=2 {
                for x in -2..=2 {
                    if bits & (1 << (x + z * 5 + 12)) != 0 {
                        holder.set_neighbour(x, z, true);
                    }
                }
            }
            assert_eq!(
                holder.pending() as u64,
                row["pending"].as_u64().unwrap(),
                "{line}"
            );
            assert_eq!(
                holder.current(),
                FullChunkStatus::Inaccessible,
                "no callback effects have run"
            );
            count += 1;
        }
        assert_eq!(count, 120);
    }
}
