//! Port of `net.minecraft.tags.TagNetworkSerialization.NetworkPayload` (issue
//! #109) — the per-registry tag map wire shape carried by
//! `ClientboundUpdateTagsPacket`.
//!
//! Java: `TagNetworkSerialization.java` (nested `NetworkPayload` class) in
//! `working/Paper`. A `Map<Identifier, IntList>` — for each tag, the registry's
//! element ids (`registry.getId(holder.value())`, the registry's `Int2ObjectMap`).
//!
//! The wire shape is `buf.writeMap(this.tags, FriendlyByteBuf::writeIdentifier,
//! FriendlyByteBuf::writeIntIdList)` — a varint map count, then per tag a
//! **raw identifier string** (`writeUtf`), then a varint `IntList` count + the
//! varint ids. The `Identifier` string is raw (Java `writeIdentifier` calls the
//! *unbounded* `writeUtf(identifier.toString())`, not the codec boundary), so a
//! hostile identifier here panics like every raw `FriendlyByteBuf::readIdentifier`
//! (netty/`Identifier.parse` exception) — the codec boundary is not involved.
//!
//! The outer `readMap` uses `Maps::newHashMapWithExpectedSize`, whose
//! `capacity` calls guava `checkNonnegative` — a negative count panics with
//! `"expectedSize cannot be negative but was: {n}"`. `readIntIdList` allocates
//! an empty `IntArrayList` and appends, so it has no negative-count panic; the
//! Rust preallocation is capped to the wire count to stay equivalent (a hostile
//! huge count loops and aborts on truncation instead of over-allocating).
//!
//! Map iteration order on the wire is not contractually stable (Java `HashMap`),
//! and the per-tag `IntList` is inherently ordered; tests compare decoded
//! content, never byte identity, for multi-tag maps (capture-semantics rule).
//! The captured golden body is byte-compared only for the trailing packet that
//! is provably stable.

use crate::friendly_byte_buf::{FriendlyByteBuf, MAX_STRING_LENGTH};
use rivet_registry::Identifier;

use std::collections::HashMap;

/// `TagNetworkSerialization.NetworkPayload.EMPTY` — `new NetworkPayload(Map.of())`.
///
/// A fresh empty payload (Java's static `EMPTY` is a shared instance; this is
/// an owned equivalent — the value is a `HashMap`, not const-constructible, so
/// it is a function, and callers hold it by value).
pub fn empty() -> NetworkPayload {
    NetworkPayload {
        tags: HashMap::new(),
    }
}

/// `TagNetworkSerialization.NetworkPayload` — `(Map<Identifier, IntList> tags)`.
///
/// A `HashMap` so the map keys mirror Java's `HashMap`; the `Identifier` keys
/// use the Java-parity hash. Wire order is arbitrary (Java `map.forEach`).
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkPayload {
    /// `tags` — each tag's location identifier -> the ordered list of registry
    /// element ids (the `IntList`).
    tags: HashMap<Identifier, Vec<i32>>,
}

impl NetworkPayload {
    /// `new NetworkPayload(Map<Identifier, IntList> tags)`.
    pub fn new(tags: HashMap<Identifier, Vec<i32>>) -> Self {
        NetworkPayload { tags }
    }

    /// `NetworkPayload.tags()`.
    pub fn tags(&self) -> &HashMap<Identifier, Vec<i32>> {
        &self.tags
    }

    /// `NetworkPayload.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// `NetworkPayload.size()`.
    pub fn size(&self) -> usize {
        self.tags.len()
    }

    /// `NetworkPayload.write(FriendlyByteBuf)` — `writeMap(tags,
    /// writeIdentifier, writeIntIdList)`.
    pub fn write(&self, buf: &mut FriendlyByteBuf) {
        // `writeMap` iterates in the map's own order (arbitrary).
        buf.write_var_int(self.tags.len() as i32);
        for (tag, ids) in &self.tags {
            buf.write_utf(&tag.to_string());
            // `writeIntIdList` — count then ids.
            buf.write_var_int(ids.len() as i32);
            for id in ids {
                buf.write_var_int(*id);
            }
        }
    }

    /// `NetworkPayload.read(FriendlyByteBuf)` — `readMap(readIdentifier,
    /// readIntIdList)`.
    pub fn read(buf: &mut FriendlyByteBuf) -> NetworkPayload {
        let count = buf.read_var_int();
        // `readMap(Maps::newHashMapWithExpectedSize, ...)` — guava's
        // `checkNonnegative` on the expected size.
        if count < 0 {
            panic!("expectedSize cannot be negative but was: {count}");
        }
        let mut tags = HashMap::with_capacity(count as usize);
        for _ in 0..count {
            // `readIdentifier` -> `Identifier.parse` (raw, panics on a hostile
            // id — Java `IdentifierException`).
            let tag = Identifier::parse(&buf.read_utf_max(MAX_STRING_LENGTH));
            // `readIntIdList` — an empty `IntArrayList` appended to; a negative
            // count therefore does not panic, and preallocation mirrors Java's
            // append growth (never over-allocates).
            let id_count = buf.read_var_int();
            let mut ids = Vec::new();
            for _ in 0..id_count {
                ids.push(buf.read_var_int());
            }
            tags.insert(tag, ids);
        }
        NetworkPayload { tags }
    }
}
