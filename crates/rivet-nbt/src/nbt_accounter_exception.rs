//! Port of `net.minecraft.nbt.NbtAccounterException` — `extends NbtException`.

/// `NbtAccounterException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NbtAccounterException {
    pub message: String,
}

impl NbtAccounterException {
    pub fn new(message: impl Into<String>) -> Self {
        NbtAccounterException {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for NbtAccounterException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for NbtAccounterException {}
