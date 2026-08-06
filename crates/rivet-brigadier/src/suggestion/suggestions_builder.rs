//! Port of `com.mojang.brigadier.suggestion.SuggestionsBuilder` (upstream
//! brigadier-1.3.10).

use crate::Message;
use crate::context::StringRange;
use crate::suggestion::Suggestions;
use crate::suggestion::suggestion::Suggestion;
use std::sync::Arc;

/// Java `SuggestionsBuilder` — accumulates `Suggestion`s over a truncated input.
#[derive(Debug, Clone)]
pub struct SuggestionsBuilder {
    input: String,
    input_lower_case: String,
    start: i32,
    remaining: String,
    remaining_lower_case: String,
    result: Vec<Suggestion>,
}

impl SuggestionsBuilder {
    /// Java `SuggestionsBuilder(String input, String inputLowerCase, int start)`.
    pub fn new(input: String, input_lower_case: String, start: i32) -> Self {
        let remaining = substring_utf16(&input, start);
        let remaining_lower_case = substring_utf16(&input_lower_case, start);
        SuggestionsBuilder {
            input,
            input_lower_case,
            start,
            remaining,
            remaining_lower_case,
            result: Vec::new(),
        }
    }

    /// Java `SuggestionsBuilder(String input, int start)`.
    pub fn new_with_input(input: String, start: i32) -> Self {
        let input_lower_case = input.to_lowercase();
        SuggestionsBuilder::new(input, input_lower_case, start)
    }

    /// Java `getInput()`.
    pub fn get_input(&self) -> &str {
        &self.input
    }

    /// Java `getStart()`.
    pub fn get_start(&self) -> i32 {
        self.start
    }

    /// Java `getRemaining()`.
    pub fn get_remaining(&self) -> &str {
        &self.remaining
    }

    /// Java `getRemainingLowerCase()`.
    pub fn get_remaining_lower_case(&self) -> &str {
        &self.remaining_lower_case
    }

    /// Java `build()`.
    pub fn build(&self) -> Suggestions {
        Suggestions::create(&self.input, &self.result)
    }

    /// Java `suggest(String)` — a no-op when the text equals the remaining input.
    pub fn suggest(&mut self, text: &str) -> &mut Self {
        if text == self.remaining {
            return self;
        }
        self.result.push(Suggestion::new(
            StringRange::between(self.start, self.input_len()),
            text,
        ));
        self
    }

    /// Java `suggest(String, Message)`.
    pub fn suggest_with_tooltip(&mut self, text: &str, tooltip: Arc<dyn Message>) -> &mut Self {
        if text == self.remaining {
            return self;
        }
        self.result.push(Suggestion::new_with_tooltip(
            StringRange::between(self.start, self.input_len()),
            text,
            tooltip,
        ));
        self
    }

    /// Java `suggest(int)` — an integer suggestion.
    pub fn suggest_int(&mut self, value: i32) -> &mut Self {
        self.result.push(Suggestion::integer(
            StringRange::between(self.start, self.input_len()),
            value,
        ));
        self
    }

    /// Java `suggest(int, Message)`.
    pub fn suggest_int_with_tooltip(&mut self, value: i32, tooltip: Arc<dyn Message>) -> &mut Self {
        self.result.push(Suggestion::integer_with_tooltip(
            StringRange::between(self.start, self.input_len()),
            value,
            tooltip,
        ));
        self
    }

    /// Java `add(SuggestionsBuilder)` — append the other builder's suggestions.
    pub fn add(&mut self, other: &SuggestionsBuilder) -> &mut Self {
        self.result.extend(other.result.iter().cloned());
        self
    }

    /// Java `createOffset(int)`.
    pub fn create_offset(&self, start: i32) -> SuggestionsBuilder {
        SuggestionsBuilder::new(self.input.clone(), self.input_lower_case.clone(), start)
    }

    /// Java `restart()`.
    pub fn restart(&self) -> SuggestionsBuilder {
        self.create_offset(self.start)
    }

    fn input_len(&self) -> i32 {
        self.input.encode_utf16().count() as i32
    }
}

/// `input.substring(start)` in UTF-16 code units.
fn substring_utf16(input: &str, start: i32) -> String {
    let units: Vec<u16> = input.encode_utf16().collect();
    let start = i32::min(i32::max(0, start), units.len() as i32) as usize;
    crate::immutable_string_reader::utf16_units_to_string(&units[start..])
}
