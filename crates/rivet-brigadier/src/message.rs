//! Port of `com.mojang.brigadier.Message` (upstream).

/// Java `Message` interface — a human-readable message with `getString()`.
pub trait Message: Send + Sync {
    fn get_string(&self) -> &str;
}

impl Message for String {
    fn get_string(&self) -> &str {
        self
    }
}

impl Message for &str {
    fn get_string(&self) -> &str {
        self
    }
}
