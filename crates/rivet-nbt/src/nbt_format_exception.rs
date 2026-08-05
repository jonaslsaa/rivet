//! Port of `net.minecraft.nbt.NbtFormatException` — `extends NbtException`.

use crate::nbt_exception::NbtException;

/// `NbtFormatException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NbtFormatException {
    pub message: String,
}

impl NbtFormatException {
    pub fn new(message: impl Into<String>) -> Self {
        NbtFormatException {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for NbtFormatException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for NbtFormatException {}

impl From<NbtFormatException> for NbtException {
    fn from(e: NbtFormatException) -> Self {
        NbtException::new(e.message)
    }
}
