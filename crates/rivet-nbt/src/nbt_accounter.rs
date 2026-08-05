//! Port of `net.minecraft.nbt.NbtAccounter` — usage/stack-depth accounting.

use crate::nbt_accounter_exception::NbtAccounterException;

pub const DEFAULT_NBT_QUOTA: i64 = 2_097_152;
pub const UNCOMPRESSED_NBT_QUOTA: i64 = 104_857_600;
const MAX_STACK_DEPTH: i32 = 512;

/// `NbtAccounter`.
#[derive(Debug, Clone)]
pub struct NbtAccounter {
    quota: i64,
    usage: i64,
    max_depth: i32,
    depth: i32,
}

impl NbtAccounter {
    /// `new NbtAccounter(quota, maxDepth)`.
    pub fn new(quota: i64, max_depth: i32) -> Self {
        NbtAccounter {
            quota,
            usage: 0,
            max_depth,
            depth: 0,
        }
    }

    /// `NbtAccounter.create(quota)`.
    pub fn create(quota: i64) -> Self {
        NbtAccounter::new(quota, MAX_STACK_DEPTH)
    }

    /// `NbtAccounter.defaultQuota()`.
    pub fn default_quota() -> Self {
        NbtAccounter::new(DEFAULT_NBT_QUOTA, MAX_STACK_DEPTH)
    }

    /// `NbtAccounter.uncompressedQuota()`.
    pub fn uncompressed_quota() -> Self {
        NbtAccounter::new(UNCOMPRESSED_NBT_QUOTA, MAX_STACK_DEPTH)
    }

    /// `NbtAccounter.unlimitedHeap()`.
    pub fn unlimited_heap() -> Self {
        NbtAccounter::new(i64::MAX, MAX_STACK_DEPTH)
    }

    /// `accountBytes(bytesPerEntry * count)`.
    pub fn account_bytes_per_entry(&mut self, bytes_per_entry: i64, count: i64) {
        self.account_bytes(bytes_per_entry.wrapping_mul(count));
    }

    /// `accountBytes(size)`.
    pub fn account_bytes(&mut self, size: i64) {
        if size < 0 {
            panic!(
                "{}",
                NbtAccounterException::new(format!(
                    "Tried to account NBT tag with negative size: {size}"
                ))
            );
        }
        let new_usage = self.usage.wrapping_add(size);
        if new_usage > self.quota {
            panic!(
                "{}",
                NbtAccounterException::new(format!(
                    "Tried to read NBT tag that was too big; tried to allocate: {} + {} bytes where max allowed: {}",
                    self.usage, size, self.quota
                ))
            );
        }
        self.usage = new_usage;
    }

    /// `pushDepth(depth)` (Paper — codec depth).
    pub fn push_depth_n(&mut self, depth: i32) {
        if self.depth + depth >= self.max_depth {
            panic!(
                "{}",
                NbtAccounterException::new(format!(
                    "Tried to read NBT tag with too high complexity, depth > {}",
                    self.max_depth
                ))
            );
        }
        self.depth += depth;
    }

    /// `pushDepth()`.
    pub fn push_depth(&mut self) {
        if self.depth >= self.max_depth {
            panic!(
                "{}",
                NbtAccounterException::new(format!(
                    "Tried to read NBT tag with too high complexity, depth > {}",
                    self.max_depth
                ))
            );
        }
        self.depth += 1;
    }

    /// `popDepth()`.
    pub fn pop_depth(&mut self) {
        if self.depth <= 0 {
            panic!(
                "{}",
                NbtAccounterException::new("NBT-Accounter tried to pop stack-depth at top-level")
            );
        }
        self.depth -= 1;
    }

    /// `getUsage()` (VisibleForTesting).
    pub fn get_usage(&self) -> i64 {
        self.usage
    }

    /// `getDepth()` (VisibleForTesting).
    pub fn get_depth(&self) -> i32 {
        self.depth
    }
}
