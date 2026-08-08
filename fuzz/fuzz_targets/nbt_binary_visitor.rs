//! Fuzz target: the streaming binary NBT reader (`NbtIo.parse` /
//! `parse_compressed`) with a visitor that always returns `Continue`, forcing
//! full traversal of the tag tree.
//!
//! This exercises `skip_string`, `skip`, `parse_tag`, `parse_list`,
//! `parse_compound` and the nested-tag parse paths — a distinct dispatch path
//! (`TagType.parse`) from the `read` family, where a buggy visitor or a
//! traversal bug could otherwise hide.
//!
//! As in `nbt_binary`, the read path panics on inputs that crash Java's parse
//! (missing list element type, negative list length, oversized array,
//! `NbtAccounter` quota/depth) — all faithful expected outcomes, swallowed by
//! `catch_unwind`. The accounter is bounded to the server's 2 MiB default quota.
#![no_main]
use libfuzzer_sys::fuzz_target;

mod common;
use common::guarded;
use rivet_fuzz::targets::{NBT_BINARY_VISITOR_STEPS, nbt_binary_visitor_step};

fuzz_target!(|data: &[u8]| {
    for step in 0..NBT_BINARY_VISITOR_STEPS {
        guarded(|| nbt_binary_visitor_step(data, step));
    }
});
