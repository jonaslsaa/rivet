//! Port of `com.mojang.brigadier.suggestion.Suggestions` (upstream brigadier-1.3.10).

use crate::context::StringRange;
use crate::suggestion::suggestion::Suggestion;

/// Java `Suggestions` — a range plus a sorted list of suggestions.
#[derive(Debug, Clone)]
pub struct Suggestions {
    range: StringRange,
    suggestions: Vec<Suggestion>,
}

impl Suggestions {
    /// Java `Suggestions(StringRange, List<Suggestion>)`.
    pub fn new(range: StringRange, suggestions: Vec<Suggestion>) -> Self {
        Suggestions { range, suggestions }
    }

    /// Java `getRange()`.
    pub fn get_range(&self) -> StringRange {
        self.range
    }

    /// Java `getList()`.
    pub fn get_list(&self) -> &[Suggestion] {
        &self.suggestions
    }

    /// Java `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.suggestions.is_empty()
    }

    /// Java `Suggestions.empty()`.
    pub fn empty() -> Suggestions {
        Suggestions {
            range: StringRange::at(0),
            suggestions: Vec::new(),
        }
    }

    /// Java `merge(String, Collection<Suggestions>)`.
    pub fn merge(command: &str, input: &[Suggestions]) -> Suggestions {
        if input.is_empty() {
            return Suggestions::empty();
        } else if input.len() == 1 {
            return input[0].clone();
        }

        let mut texts: Vec<Suggestion> = Vec::new();
        for suggestions in input {
            texts.extend(suggestions.get_list().iter().cloned());
        }
        Suggestions::create(command, &texts)
    }

    /// Java `create(String, Collection<Suggestion>)`.
    pub fn create(command: &str, suggestions: &[Suggestion]) -> Suggestions {
        if suggestions.is_empty() {
            return Suggestions::empty();
        }
        let mut start = i32::MAX;
        let mut end = i32::MIN;
        for suggestion in suggestions {
            start = i32::min(suggestion.get_range().get_start(), start);
            end = i32::max(suggestion.get_range().get_end(), end);
        }
        let range = StringRange::new(start, end);
        let mut texts: Vec<Suggestion> = Vec::new();
        for suggestion in suggestions {
            texts.push(suggestion.expand(command, range));
        }
        // Java sorts with `List.sort` (TimSort). The Paper-patched
        // `compareToIgnoreCase` is transitive (any integer sorts before any text), so
        // the order is deterministic and Rust's stable merge sort reproduces it.
        texts.sort_by(|a, b| a.compare_to_ignore_case(b));
        // Java dedups via a HashSet before sorting; equal suggestions collapse. After
        // expand, equal elements are adjacent under the total order, so a dedup pass
        // reproduces the set.
        texts.dedup();
        Suggestions {
            range,
            suggestions: texts,
        }
    }

    /// Java `hashCode()` — `Objects.hash(range, suggestions)`.
    pub fn hash_code(&self) -> i32 {
        let list_hash = self.suggestions.iter().fold(1i32, |acc, s| {
            31_i32.wrapping_mul(acc).wrapping_add(s.hash_code())
        });
        crate::java_hash::objects_hash(&[self.range.hash_code(), list_hash])
    }
}

/// Java `equals`: `Objects.equals(range, suggestions)`.
impl PartialEq for Suggestions {
    fn eq(&self, other: &Self) -> bool {
        self.range == other.range && self.suggestions == other.suggestions
    }
}

impl Eq for Suggestions {}
