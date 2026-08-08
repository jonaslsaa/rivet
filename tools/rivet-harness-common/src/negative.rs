//! Named-path negative-control helpers.
//!
//! A negative control (`--expect-fail`, `--mutate`) proves a comparison
//! pipeline is not vacuously green: it injects a known defect into a *copy* of
//! a baseline and requires the diff to detect *and name* it. The naming is the
//! load-bearing part — a clean diff, or a divergence naming something other
//! than the tampered item, must fail the control. These helpers own the
//! bookkeeping both harness tools already duplicated.

use std::fmt;
use std::io;

/// The verdict of a negative-control run.
#[derive(Debug)]
pub enum Verdict {
    /// The tampered item was detected and named.
    Detected(String),
    /// The diff was clean: the pipeline never saw the injected defect.
    Clean(String),
    /// The diff diverged but named something other than the tampered item.
    WrongItem {
        tampered: String,
        mismatched: Vec<String>,
    },
}

impl Verdict {
    /// Whether the negative control passed.
    pub fn passed(&self) -> bool {
        matches!(self, Verdict::Detected(_))
    }

    /// The tampered item, if this verdict was produced from one.
    pub fn tampered(&self) -> Option<&str> {
        match self {
            Verdict::Detected(t) => Some(t),
            Verdict::Clean(t) => Some(t),
            Verdict::WrongItem { tampered, .. } => Some(tampered),
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Detected(t) => write!(f, "detected and named {t}"),
            Verdict::Clean(t) => write!(f, "zero divergence despite tampered {t}"),
            Verdict::WrongItem {
                tampered,
                mismatched,
            } => write!(f, "diverged but named {mismatched:?} instead of {tampered}"),
        }
    }
}

/// Build the verdict for a named-path negative control: `mismatched` is the
/// list of items the pipeline reported as diverging; the control passes only
/// when it contains the tampered path. `empty_on_none` reports a *clean* diff
/// (false negative) when no items diverge.
pub fn verdict(tampered: &str, mismatched: &[String]) -> Verdict {
    if mismatched.iter().any(|p| p == tampered) {
        Verdict::Detected(tampered.to_owned())
    } else if mismatched.is_empty() {
        Verdict::Clean(tampered.to_owned())
    } else {
        Verdict::WrongItem {
            tampered: tampered.to_owned(),
            mismatched: mismatched.to_vec(),
        }
    }
}

/// Recursively copy a directory tree (used so a tamper never touches the
/// committed baseline).
pub fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_when_the_tampered_path_is_named() {
        let v = verdict("region/r.0.0.mca", &["region/r.0.0.mca".to_owned()]);
        assert!(v.passed(), "{v}");
        assert_eq!(v.tampered(), Some("region/r.0.0.mca"));
    }

    #[test]
    fn clean_when_nothing_diverges() {
        let v = verdict("region/r.0.0.mca", &[]);
        assert!(!v.passed(), "a clean diff is a false negative");
        assert!(matches!(v, Verdict::Clean(_)));
    }

    #[test]
    fn wrong_item_when_the_diff_names_something_else() {
        let v = verdict("region/r.0.0.mca", &["region/r.1.0.mca".to_owned()]);
        assert!(
            !v.passed(),
            "an unrelated divergence must not satisfy the control"
        );
        match v {
            Verdict::WrongItem {
                tampered,
                mismatched,
            } => {
                assert_eq!(tampered, "region/r.0.0.mca");
                assert_eq!(mismatched, vec!["region/r.1.0.mca".to_owned()]);
            }
            other => panic!("expected WrongItem, got {other}"),
        }
    }

    #[test]
    fn copy_dir_recursive_mirrors_a_tree() {
        let base = std::env::temp_dir().join(format!("rivet-hc-neg-{}", std::process::id()));
        let src = base.join("src");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("a.txt"), "a").unwrap();
        std::fs::write(src.join("nested/b.txt"), "b").unwrap();
        let dst = base.join("dst");
        copy_dir_recursive(&src, &dst).unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "a");
        assert_eq!(
            std::fs::read_to_string(dst.join("nested/b.txt")).unwrap(),
            "b"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }
}
