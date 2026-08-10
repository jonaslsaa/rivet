//! Port of `io.papermc.paper.plugin.loader.library.LibraryLoadingException`
//! (Paper 26.2).

/// Java `LibraryLoadingException extends RuntimeException`.
///
/// "Indicates that an exception has occurred while loading a library." The
/// Java class is `RuntimeException` (unchecked), so it is raised with
/// `panic!` at call sites where Paper lets it propagate, and carried as
/// `Result::Err` where Paper handles it. Message text and the optional
/// causing exception are preserved exactly.
#[derive(Debug)]
pub struct LibraryLoadingException {
    message: String,
    /// The `LibraryLoadingException(String, Exception)` cause, `None` for the
    /// single-argument constructor. Java stores it in the `Throwable` cause
    /// field; the port keeps it as a message-carrying boxed error.
    cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl LibraryLoadingException {
    /// Java `LibraryLoadingException(String s)` — `super(s)`, no cause.
    pub fn new(message: String) -> Self {
        LibraryLoadingException {
            message,
            cause: None,
        }
    }

    /// Java `LibraryLoadingException(String s, Exception e)` — `super(s, e)`,
    /// retaining the cause.
    pub fn new_with_cause(
        message: String,
        cause: Box<dyn std::error::Error + Send + Sync>,
    ) -> Self {
        LibraryLoadingException {
            message,
            cause: Some(cause),
        }
    }

    /// Java `Throwable.getMessage()` — the message passed to the constructor.
    pub fn get_message(&self) -> &str {
        &self.message
    }

    /// Java `Throwable.getCause()` — the causing exception, if any.
    pub fn get_cause(&self) -> Option<&(dyn std::error::Error + Send + Sync)> {
        self.cause.as_deref()
    }
}

impl std::fmt::Display for LibraryLoadingException {
    /// Java `Throwable.toString()` — `"LibraryLoadingException: <message>"`,
    /// the first stack-trace line Paper surfaces when a plugin's library load
    /// fails. `get_message()` stays raw; this is the display form.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LibraryLoadingException: {}", self.message)
    }
}

impl std::error::Error for LibraryLoadingException {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause
            .as_deref()
            .map(|cause| cause as &(dyn std::error::Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    /// The single-argument constructor carries the exact message and no cause.
    #[test]
    fn message_only_constructor_preserves_text_and_has_no_cause() {
        let error =
            LibraryLoadingException::new("Could not find library at libs/missing.jar".to_string());
        assert_eq!(
            error.get_message(),
            "Could not find library at libs/missing.jar"
        );
        assert!(error.get_cause().is_none());
        // Display mirrors Java `Throwable.toString()`: class name, colon,
        // message — the line Paper surfaces when a plugin library load fails.
        assert_eq!(
            error.to_string(),
            "LibraryLoadingException: Could not find library at libs/missing.jar"
        );
        assert!(error.source().is_none());
    }

    /// The two-argument constructor preserves both the exact message and the
    /// exact cause (`Throwable`'s message/cause are kept verbatim in Java).
    #[test]
    fn two_argument_constructor_keeps_message_and_cause() {
        let cause: Box<dyn std::error::Error + Send + Sync> =
            Box::new(std::io::Error::other("connection reset"));
        let error = LibraryLoadingException::new_with_cause(
            "Failed to resolve dependency".to_string(),
            cause,
        );
        assert_eq!(error.get_message(), "Failed to resolve dependency");
        let cause = error.get_cause().expect("cause should be retained");
        assert_eq!(cause.to_string(), "connection reset");
        // `Error::source` surfaces the same cause.
        let source = error.source().expect("source should be the cause");
        assert_eq!(source.to_string(), "connection reset");
    }
}
