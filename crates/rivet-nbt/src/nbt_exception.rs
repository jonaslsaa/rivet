//! Port of `net.minecraft.nbt.NbtException` — `extends RuntimeException`.

/// Java unchecked `RuntimeException` subclass. Per PORTING.md these map to a
/// panic where vanilla crashing is the observable behavior (parsing path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NbtException {
    pub message: String,
}

impl NbtException {
    pub fn new(message: impl Into<String>) -> Self {
        NbtException {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for NbtException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for NbtException {}

impl std::panic::UnwindSafe for NbtException {}
impl std::panic::RefUnwindSafe for NbtException {}
