//! Pinned Paper 0a99345 cache values, emitted by the actual
//! BlockMutationMetadataProbe (run_block_mutation_metadata_probe.sh).
//! Bits: CollisionUtil.isSpecialCollidingBlock, FluidState.isRandomlyTicking,
//! BlockState.hasAnalogOutputSignal. The contiguous runs cover every StateId.
use crate::generated::block_states::{BLOCK_STATE_COUNT, StateId};
use std::sync::LazyLock;

static FLAGS: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut flags = Vec::with_capacity(usize::from(BLOCK_STATE_COUNT));
    for line in include_str!("section_flags.tsv").lines() {
        let values: Vec<usize> = line.split('\t').map(|x| x.parse().unwrap()).collect();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], flags.len());
        flags.resize(flags.len() + values[1], values[2] as u8);
    }
    assert_eq!(flags.len(), usize::from(BLOCK_STATE_COUNT));
    flags
});

pub(super) fn flags(state: StateId) -> u8 {
    // Like the other pure BlockState operations, an invalid dense ID resolves
    // to air. Such an ID cannot arise from the actual Java BlockState type.
    FLAGS.get(usize::from(state.0)).copied().unwrap_or(0)
}
