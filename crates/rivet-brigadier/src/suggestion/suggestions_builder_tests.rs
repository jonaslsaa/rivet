//! Unit tests ported from the upstream brigadier `SuggestionsBuilderTest` (MIT).

use crate::context::StringRange;
use crate::suggestion::{Suggestion, SuggestionsBuilder};

#[test]
fn suggest_appends() {
    let mut builder = SuggestionsBuilder::new_with_input("Hello w".to_string(), 6);
    let result = builder.suggest("world!").build();
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::between(6, 7), "world!")]
    );
    assert_eq!(result.get_range(), StringRange::between(6, 7));
    assert!(!result.is_empty());
}

#[test]
fn suggest_replaces() {
    let mut builder = SuggestionsBuilder::new_with_input("Hello w".to_string(), 6);
    let result = builder.suggest("everybody").build();
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::between(6, 7), "everybody")]
    );
    assert_eq!(result.get_range(), StringRange::between(6, 7));
    assert!(!result.is_empty());
}

#[test]
fn suggest_noop() {
    let mut builder = SuggestionsBuilder::new_with_input("Hello w".to_string(), 6);
    let result = builder.suggest("w").build();
    assert_eq!(result.get_list(), &[]);
    assert!(result.is_empty());
}

#[test]
fn suggest_multiple() {
    let mut builder = SuggestionsBuilder::new_with_input("Hello w".to_string(), 6);
    let result = builder
        .suggest("world!")
        .suggest("everybody")
        .suggest("weekend")
        .build();
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::between(6, 7), "everybody"),
            Suggestion::new(StringRange::between(6, 7), "weekend"),
            Suggestion::new(StringRange::between(6, 7), "world!"),
        ]
    );
    assert_eq!(result.get_range(), StringRange::between(6, 7));
    assert!(!result.is_empty());
}

#[test]
fn restart() {
    let mut builder = SuggestionsBuilder::new_with_input("Hello w".to_string(), 6);
    builder.suggest("won't be included in restart");
    let other = builder.restart();
    // Java's `is(not(builder))` — a new instance with the same input/start/remaining
    // (the accumulated suggestions are dropped). The value equality on the derived
    // fields is what the ported test observes.
    assert_eq!(other.get_input(), builder.get_input());
    assert_eq!(other.get_start(), builder.get_start());
    assert_eq!(other.get_remaining(), builder.get_remaining());
    // restart() discards the accumulated suggestions.
    assert_eq!(other.build().get_list(), &[]);
}

#[test]
fn sort_alphabetical() {
    let mut builder = SuggestionsBuilder::new_with_input("Hello w".to_string(), 6);
    let result = builder
        .suggest("2")
        .suggest("4")
        .suggest("6")
        .suggest("8")
        .suggest("30")
        .suggest("32")
        .build();
    let actual: Vec<String> = result
        .get_list()
        .iter()
        .map(|s| s.get_text().to_string())
        .collect();
    assert_eq!(actual, ["2", "30", "32", "4", "6", "8"]);
}

#[test]
fn sort_numerical() {
    let mut builder = SuggestionsBuilder::new_with_input("Hello w".to_string(), 6);
    let result = builder
        .suggest_int(2)
        .suggest_int(4)
        .suggest_int(6)
        .suggest_int(8)
        .suggest_int(30)
        .suggest_int(32)
        .build();
    let actual: Vec<String> = result
        .get_list()
        .iter()
        .map(|s| s.get_text().to_string())
        .collect();
    assert_eq!(actual, ["2", "4", "6", "8", "30", "32"]);
}

#[test]
fn sort_mixed() {
    let mut builder = SuggestionsBuilder::new_with_input("Hello w".to_string(), 6);
    let result = builder
        .suggest("11")
        .suggest("22")
        .suggest("33")
        .suggest("a")
        .suggest("b")
        .suggest("c")
        .suggest_int(2)
        .suggest_int(4)
        .suggest_int(6)
        .suggest_int(8)
        .suggest_int(30)
        .suggest_int(32)
        .suggest("3a")
        .suggest("a3")
        .build();
    let actual: Vec<String> = result
        .get_list()
        .iter()
        .map(|s| s.get_text().to_string())
        .collect();
    // Paper's `compareToIgnoreCase` sorts every integer suggestion before any text
    // suggestion, then the text case-insensitively — the mixed order is deterministic
    // (upstream 1.3.10's non-transitive comparator is replaced by Paper's `compare0`).
    assert_eq!(
        actual,
        [
            "2", "4", "6", "8", "30", "32", "11", "22", "33", "3a", "a", "a3", "b", "c"
        ]
    );
}
